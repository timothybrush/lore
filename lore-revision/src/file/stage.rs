// SPDX-FileCopyrightText: 2026 Epic Games, Inc.
// SPDX-License-Identifier: MIT
use std::sync::Arc;
use std::sync::atomic::Ordering;

use lore_base::lore_spawn;
use lore_error_set::prelude::*;
use tokio::task::JoinSet;

use crate::event;
use crate::filter::FilterMode;
use crate::hash::hash_string;
use crate::interface::LoreArray;
use crate::interface::LoreString;
use crate::layer;
use crate::link::LinkTracker;
use crate::lore::Hash;
use crate::lore::execution_context;
use crate::lore_debug;
use crate::node::Node;
use crate::node::NodeBlock;
use crate::node::NodeFlags;
use crate::node::ROOT_NODE;
use crate::node::SiblingCycleGuard;
use crate::path::emit_path_ignore;
use crate::repository::RepositoryContext;
use crate::repository::RepositoryWriteToken;
use crate::stage;
use crate::stage::LoreFileStageBeginEventData;
use crate::stage::LoreFileStageCountData;
use crate::stage::LoreFileStageEndEventData;
use crate::stage::LoreFileStageProgressEventData;
use crate::stage::LoreFileStageRevisionEventData;
use crate::stage::StageError;
use crate::stage::StageOptions;
use crate::stage::StageStats;
use crate::state;
use crate::state::State;
use crate::util::path::RelativePath;
use crate::util::path::RelativePathBuf;

/// Spawn a stage task into the given layer's repository covering `remain` (the
/// path-suffix relative to the layer's mount). An empty `remain` stages the
/// layer's whole subtree.
async fn stage_into_single_layer(
    tasks: &mut JoinSet<Result<crate::node::NodeLink, StageError>>,
    layer: &crate::layer::Layer,
    layer_state: &crate::layer::LayerState,
    parent_repository: Arc<RepositoryContext>,
    remain: &str,
    stats: Arc<StageStats>,
    options: StageOptions,
) -> Result<(), StageError> {
    let absolute_path = parent_repository.require_path()?.join(&layer.target_path);

    let layer_relative_path = RelativePathBuf::new_from_initial_path(&layer.source_path)
        .forward::<StageError>("Failed to construct layer relative path")?;
    let remain_relative_path = if remain.is_empty() {
        RelativePath::new()
    } else {
        RelativePath::new_from_initial_path(remain).unwrap_or_default()
    };

    // TODO(mjansson): If this has gone past a link into a subrepository, we
    // need to stage the link node and upwards in the layer repository.
    let layer_staged_node = layer_state
        .state_staged
        .find_node_link(layer_state.repository.clone(), layer_relative_path.as_str())
        .await
        .forward::<StageError>("Failed to locate layer source base node")?;

    let (layer_repository, layer_state_staged) = layer_staged_node
        .resolve(
            layer_state.repository.clone(),
            layer_state.state_staged.clone(),
        )
        .await
        .forward::<StageError>("Failed to locate layer source base node")?;

    lore_debug!(
        "Staging path in layer {}: {} / {}",
        layer.target_path,
        layer.source_path,
        remain_relative_path
    );

    lore_spawn!(
        tasks,
        stage::stage_filesystem_path(
            layer_repository,
            layer_state_staged,
            absolute_path,
            layer_relative_path,
            layer_staged_node.node,
            remain_relative_path,
            stats,
            options,
            None, // No link tracking in layer staging
            None, // Layers don't have nested layer mounts (no overlap)
        )
    );

    Ok(())
}

async fn try_stage_path(
    repository: Arc<RepositoryContext>,
    path: &LoreString,
) -> Option<RelativePath> {
    let repository_path = repository.require_path().ok()?;
    let Ok(relative_path) = RelativePath::new_from_user_path(repository_path, path.as_str()) else {
        emit_path_ignore(path.as_str()).await;
        lore_debug!("Ignoring invalid path: {path}");
        return None;
    };
    Some(relative_path)
}

/// Stage `paths` into the staged revision and return its hash.
///
/// Each entry in `paths` is classified as either an individual file path or
/// a directory path (the repository root counts as a directory):
///
/// - **Individual file paths** are always reconciled against the filesystem.
///   The file is read and its current state is staged regardless of dirty
///   flags. [`StageOptions::scan`] has no effect on these paths.
/// - **Directory paths** by default stage only the files and child
///   directories currently marked dirty in the repository state — this is
///   the fast path and relies on prior notifications or `status --scan`
///   calls to keep dirty flags accurate. When [`StageOptions::scan`] is
///   `true`, the directory is walked recursively on the filesystem, every
///   contained file is reconciled, and the dirty flags are disregarded.
pub async fn stage(
    repository: Arc<RepositoryContext>,
    token: &RepositoryWriteToken,
    paths: LoreArray<LoreString>,
    options: StageOptions,
) -> Result<Hash, StageError> {
    let (state_current, state_staged, _branch) =
        State::deserialize_current_and_staged(repository.clone())
            .await
            .forward::<StageError>("Failed to deserialize revision state")?;
    // Save the current revision before any modifications — the staged state
    // may share the same Arc<State> and modifications would change both.
    let current_revision = state_current.revision();
    let state = state_staged.unwrap_or_else(|| state_current.clone());

    let layers = {
        let mut layers = vec![];
        let list = layer::list(repository.clone()).await.unwrap_or_default();
        for layer in list {
            let layer_state = layer
                .deserialize_current_and_staged(repository.clone())
                .await
                .internal("Failed to deserialize layer state")?;

            layers.push((layer, layer_state));
        }
        layers
    };

    event::LoreEvent::FileStageBegin(LoreFileStageBeginEventData {
        path_count: paths.len(),
    })
    .send();

    lore_debug!("Stage options: {:?}", options);

    let mut ticker = tokio::time::interval(std::time::Duration::from_millis(500));
    let stats = Arc::new(StageStats::default());
    let mut layer_staged = vec![];
    let mut main_count = 0;
    let link_tracker = LinkTracker::new();

    let layer_target_paths: Vec<&str> = layers
        .iter()
        .map(|(layer, _)| layer.target_path.as_str())
        .collect();

    for path in paths.as_slice().iter() {
        let Some(relative_path) = try_stage_path(repository.clone(), path).await else {
            continue;
        };

        // TODO(mjansson): Fix parallel path staging to allow this to collect all paths
        let mut tasks = JoinSet::new();

        let route = classify_stage_path(relative_path.as_str(), &layer_target_paths);

        match route {
            LayerRoute::Inside {
                layer_index,
                remain,
            } => {
                let (layer, layer_state) = &layers[layer_index];
                stage_into_single_layer(
                    &mut tasks,
                    layer,
                    layer_state,
                    repository.clone(),
                    &remain,
                    stats.clone(),
                    options,
                )
                .await?;
                layer_staged.push((layer, layer_state));
            }
            LayerRoute::AncestorOf { layer_indices } => {
                // Stage the parent walking from the input path with the matched
                // layers' target_paths masked so we don't double-count files
                // already owned by a layer.
                let mask: Vec<String> = layer_indices
                    .iter()
                    .map(|i| layers[*i].0.target_path.clone())
                    .collect();
                let mask_arc = if mask.is_empty() {
                    None
                } else {
                    Some(Arc::new(mask))
                };

                main_count += spawn_stage_tasks(
                    &mut tasks,
                    repository.clone(),
                    state.clone(),
                    relative_path.clone(),
                    stats.clone(),
                    options,
                    link_tracker.clone(),
                    mask_arc,
                )
                .await?;

                // Plus one task per matched layer, staging the layer's whole
                // contents (no remain — the input path is above the mount).
                for layer_index in layer_indices {
                    let (layer, layer_state) = &layers[layer_index];
                    stage_into_single_layer(
                        &mut tasks,
                        layer,
                        layer_state,
                        repository.clone(),
                        "",
                        stats.clone(),
                        options,
                    )
                    .await?;
                    layer_staged.push((layer, layer_state));
                }
            }
            LayerRoute::Disjoint => {
                main_count += spawn_stage_tasks(
                    &mut tasks,
                    repository.clone(),
                    state.clone(),
                    relative_path.clone(),
                    stats.clone(),
                    options,
                    link_tracker.clone(),
                    None,
                )
                .await?;
            }
        }

        let mut failure = None;
        while !tasks.is_empty() {
            tokio::select! {
                _ = ticker.tick() => {
                    event::LoreEvent::FileStageProgress(LoreFileStageProgressEventData {
                        count: LoreFileStageCountData::new(stats.clone()),
                    }).send();
                },
                result = tasks.join_next() => {
                    if let Some(result) = result {
                        failure = failure.or(result.map_err(|e| StageError::internal_with_context(e, "Failed to join task")).flatten().err());
                    }
                }
            }
        }

        if let Some(err) = failure {
            return Err(err);
        }
    }

    let count = LoreFileStageCountData::new(stats.clone());
    let total_count = count.total_count;
    event::LoreEvent::FileStageEnd(LoreFileStageEndEventData { count }).send();

    if total_count == 0 {
        return Ok(state.revision());
    }

    let mut staged_revision = state.revision();
    // Only update parent staged metadata if the walker actually mutated the
    // parent's state. With the layer-routing dispatch a parent task may be
    // spawned for an `AncestorOf` path even when every child is a layer mount
    // (mask-skipped) and no parent files changed; in that case we must NOT
    // bump the staged anchor because the resulting hash would diverge from
    // current_revision purely from set_revision_number/set_parent_self
    // metadata writes, tricking commit into trying to commit an empty parent.
    let parent_mutated = main_count > 0 && (state.is_dirty() || link_tracker.has_modifications());
    if parent_mutated {
        // Process links that need reserialization due to downstream changes
        stage::process_link_updates(
            repository.clone(),
            token,
            state_current.clone(),
            state.clone(),
            link_tracker.clone(),
        )
        .await?;

        // Staged states should have no revision number
        state.set_revision_number(0);

        state.set_parent_self(current_revision);

        // If staged state is the initial stage based on current state, reset other parent. Otherwise
        // leave it as is, in case previous staged state was a merge/integrate
        if staged_revision == current_revision {
            state.set_parent_other(Hash::default());
            state.set_metadata_hash(Hash::default());
        }

        let signature = state
            .serialize(repository.clone(), token)
            .await
            .forward::<StageError>("Failed to serialize staged revision state")?;

        if signature != current_revision {
            staged_revision = signature;
            crate::instance::store_staged_anchor(&repository, signature)
                .await
                .forward::<StageError>("Failed to serialize staged anchor")?;
        }

        event::LoreEvent::FileStageRevision(LoreFileStageRevisionEventData {
            repository: repository.id,
            revision: signature,
        })
        .send();
    }

    for (layer, layer_state) in layer_staged {
        let state = layer_state.state_staged.clone();

        state.set_revision_number(0);

        state.set_parent_self(layer_state.state_current.revision());

        // If staged state is the initial stage based on current state, reset other parent. Otherwise
        // leave it as is, in case previous staged state was a merge/integrate
        if layer_state.state_current.revision() == layer_state.state_staged.revision() {
            state.set_parent_other(Hash::default());
            state.set_metadata_hash(Hash::default());
        }

        let signature = state
            .serialize(layer_state.repository.clone(), token)
            .await
            .forward::<StageError>("Failed to serialize staged revision state")?;

        if signature != layer.current {
            layer::store_layer_staged(
                repository.clone(),
                token,
                layer.target_path.as_str(),
                layer.repository,
                signature,
            )
            .await
            .internal("Failed to serialize new layer state")?;
        }

        lore_debug!(
            "Stored staged state {} for layer at {} currently at {}",
            signature,
            layer.target_path,
            layer.current
        );

        event::LoreEvent::FileStageRevision(LoreFileStageRevisionEventData {
            repository: layer_state.repository.id,
            revision: signature,
        })
        .send();
    }

    Ok(staged_revision)
}

/// Spawn staging tasks for a path, using dirty-based staging or filesystem walk.
///
/// When `options.scan` is false, checks if the target path is a directory in the
/// state tree. If so, collects dirty file paths under it and spawns a staging task
/// for each one. Single file paths always use the filesystem walk for backward
/// compatibility. When `options.scan` is true, always uses the filesystem walk.
///
/// Returns the number of tasks spawned for the main repository.
#[allow(clippy::too_many_arguments)]
async fn spawn_stage_tasks(
    tasks: &mut JoinSet<Result<crate::node::NodeLink, StageError>>,
    repository: Arc<RepositoryContext>,
    state: Arc<State>,
    relative_path: RelativePath,
    stats: Arc<StageStats>,
    options: StageOptions,
    link_tracker: Arc<LinkTracker>,
    layer_mask: Option<Arc<Vec<String>>>,
) -> Result<usize, StageError> {
    // When scan is not requested, try dirty-based staging for directories.
    //
    // `find_node_link` follows link mounts transparently — if the path
    // crosses one, the returned `NodeLink` references a node in the linked
    // repository. We must read the node from the state that actually owns
    // it, otherwise we'd hit a colliding block at the same coordinates in
    // the parent state and misclassify the target (e.g. a linked file would
    // appear as a parent-state directory and route through the dirty path,
    // which then collects nothing).
    if !options.scan {
        let resolved: Option<(
            Arc<State>,
            Arc<RepositoryContext>,
            crate::node::NodeID,
            bool,
        )> = if relative_path.is_empty() {
            // Root path is always a directory in the main repository.
            Some((state.clone(), repository.clone(), ROOT_NODE, true))
        } else if let Ok(node_link) = state
            .find_node_link(repository.clone(), relative_path.as_str())
            .await
            && node_link.is_valid()
        {
            let (resolved_repository, resolved_state) = if node_link.repository == repository.id {
                (repository.clone(), state.clone())
            } else {
                let linked_repository =
                    Arc::new(repository.to_link_context(node_link.repository).await);
                let linked_state =
                    State::deserialize(linked_repository.clone(), node_link.revision)
                        .await
                        .forward::<StageError>(
                            "Failed to deserialize linked state for dirty staging",
                        )?;
                (linked_repository, linked_state)
            };
            let node = resolved_state
                .node(resolved_repository.clone(), node_link.node)
                .await
                .forward::<StageError>("Failed to resolve node for dirty staging")?;
            Some((
                resolved_state,
                resolved_repository,
                node_link.node,
                node.is_directory(),
            ))
        } else {
            None
        };

        if let Some((resolved_state, resolved_repository, root_node, true)) = resolved {
            // `relative_path` is the path in the parent repository — prepend it
            // so dirty paths come back as parent-relative paths suitable for
            // the filesystem walk below, which traverses links itself.
            let dirty_paths = resolved_state
                .collect_dirty_paths(
                    resolved_repository,
                    root_node,
                    RelativePathBuf::new_from_clean_parts(relative_path.as_str(), ""),
                )
                .await
                .forward::<StageError>("Failed to collect dirty paths")?;

            if dirty_paths.is_empty() {
                return Ok(0);
            }

            let count = dirty_paths.len();
            for dirty_path in dirty_paths {
                let dirty_relative =
                    RelativePath::new_from_initial_path(dirty_path.as_str()).unwrap_or_default();
                lore_spawn!(
                    tasks,
                    stage::stage_filesystem_path(
                        repository.clone(),
                        state.clone(),
                        repository.require_path()?.to_path_buf(),
                        RelativePathBuf::new(),
                        ROOT_NODE,
                        dirty_relative,
                        stats.clone(),
                        options,
                        Some(link_tracker.clone()),
                        layer_mask.clone(),
                    )
                );
            }
            return Ok(count);
        }
    }

    // Filesystem walk: scan requested, or path is a single file
    lore_spawn!(
        tasks,
        stage::stage_filesystem_path(
            repository.clone(),
            state.clone(),
            repository.require_path()?.to_path_buf(),
            RelativePathBuf::new(),
            ROOT_NODE,
            relative_path,
            stats.clone(),
            options,
            Some(link_tracker.clone()),
            layer_mask,
        )
    );
    Ok(1)
}

/// Recursively mark all children of a directory node as moved.
/// This is called when a directory is moved to ensure all contained files
/// and subdirectories also have the move flag set.
async fn mark_children_moved(
    repository: Arc<RepositoryContext>,
    state: Arc<State>,
    parent_node: crate::node::NodeID,
    move_flag: NodeFlags,
) -> Result<(), crate::state::StateError> {
    use std::future::Future;
    use std::pin::Pin;

    fn mark_children_moved_recursive(
        repository: Arc<RepositoryContext>,
        state: Arc<State>,
        parent_node: crate::node::NodeID,
        move_flag: NodeFlags,
    ) -> Pin<Box<dyn Future<Output = Result<(), crate::state::StateError>> + Send>> {
        Box::pin(async move {
            let children = state.node_children(repository.clone(), parent_node).await?;

            for child_id in children {
                let child_node = state.node(repository.clone(), child_id).await?;

                // Determine the appropriate flag for this child
                let child_flag = if child_node.is_staged_add() {
                    NodeFlags::StagedAdd
                } else {
                    move_flag
                };

                // Mark the child node
                state
                    .node_mark(repository.clone(), child_id, child_flag, false)
                    .await?;

                // Recurse into directories
                if child_node.is_directory() {
                    mark_children_moved_recursive(
                        repository.clone(),
                        state.clone(),
                        child_id,
                        move_flag,
                    )
                    .await?;
                }
            }

            Ok(())
        })
    }

    mark_children_moved_recursive(repository, state, parent_node, move_flag).await
}

#[allow(clippy::too_many_arguments)]
pub async fn stage_merge(
    repository: Arc<RepositoryContext>,
    token: &RepositoryWriteToken,
    paths: LoreArray<LoreString>,
    options: StageOptions,
) -> Result<Hash, StageError> {
    let (state_current, state_staged, _branch) =
        state::State::deserialize_current_and_staged(repository.clone())
            .await
            .forward::<StageError>("Failed to deserialize revision state")?;
    let state_stage = state_staged.unwrap_or(state_current);

    if !state_stage.is_merge() || state_stage.revision_number() != 0 {
        return Err(StageError::internal("Not in a pending merge"));
    }

    let state_merge = State::deserialize(repository.clone(), state_stage.parent_other())
        .await
        .forward::<StageError>("Failed to deserialize revision state")?;

    event::LoreEvent::FileStageBegin(LoreFileStageBeginEventData {
        path_count: paths.len(),
    })
    .send();

    let mut ticker = tokio::time::interval(std::time::Duration::from_millis(500));
    let stats = Arc::new(StageStats::default());
    for path in paths.as_slice() {
        let Some(relative_path) = try_stage_path(repository.clone(), path).await else {
            continue;
        };

        // TODO(mjansson): Layers

        lore_debug!("Stage merge options: {:?}", options);
        let mut task = lore_spawn!(stage::stage_merge_path(
            repository.clone(),
            state_stage.clone(),
            state_merge.clone(),
            relative_path.clone(),
            stats.clone(),
            options,
            None, // TODO(vri): UCS-17955 - Merging and conflict resolution for links
        ));

        let result = loop {
            tokio::select! {
                _ = ticker.tick() => {
                    event::LoreEvent::FileStageProgress(LoreFileStageProgressEventData {
                        count: LoreFileStageCountData::new(stats.clone()),
                    }).send();
                },
                result = &mut task => {
                    break result.map_err(|e| StageError::internal_with_context(e, "Failed to join task"))?;
                }
            }
        };

        result?;
    }

    // TODO(vri): UCS-17955 - Merging and conflict resolution for links
    // Serialize all staged links states recursively

    let signature = state_stage
        .serialize(repository.clone(), token)
        .await
        .forward::<StageError>("Failed to serialize staged revision state")?;
    crate::instance::store_staged_anchor(&repository, signature)
        .await
        .forward::<StageError>("Failed to serialize staged anchor")?;

    event::LoreEvent::FileStageRevision(LoreFileStageRevisionEventData {
        repository: repository.id,
        revision: signature,
    })
    .send();

    Ok(signature)
}

#[allow(clippy::too_many_arguments)]
pub async fn stage_move(
    repository: Arc<RepositoryContext>,
    token: &RepositoryWriteToken,
    from_path: String,
    to_path: String,
    options: StageOptions,
) -> Result<Hash, StageError> {
    event::LoreEvent::FileStageBegin(LoreFileStageBeginEventData { path_count: 1 }).send();

    let from_path =
        RelativePath::new_from_user_path(repository.require_path()?, from_path.as_str())
            .forward::<StageError>(&format!("Invalid path {from_path}"))?;
    let to_path = RelativePath::new_from_user_path(repository.require_path()?, to_path.as_str())
        .forward::<StageError>(&format!("Invalid path {to_path}"))?;
    lore_debug!(
        "Stage move {} -> {} in repository {}",
        from_path.as_str(),
        to_path.as_str(),
        repository.path_for_display()
    );

    if from_path.as_str() == to_path.as_str() {
        return Err(StageError::internal("Cannot move a path to itself"));
    }

    let (state_current, state_staged, _branch) =
        State::deserialize_current_and_staged(repository.clone())
            .await
            .forward::<StageError>("Failed to deserialize revision state")?;
    // Save the current revision before any modifications — the staged state
    // may share the same Arc<State> and modifications would change both.
    let current_revision = state_current.revision();
    let state = state_staged.unwrap_or(state_current);

    if !execution_context().globals().force()
        && repository
            .filter
            .emit_excludes(&to_path, true, FilterMode::Full)
    {
        return Err(StageError::internal(format!("Ignored path {to_path}")));
    }

    // Find from node (must exist, optionally already staged for delete)
    let from_node_link = state
        .find_node_link(repository.clone(), from_path.as_str())
        .await
        .forward::<StageError>(&format!("Path {from_path} does not exist in repository "))?;
    if !from_node_link.is_valid() {
        return Err(StageError::internal(format!(
            "Path {from_path} does not exist in repository "
        )));
    }

    let from_node = state
        .node(repository.clone(), from_node_link.node)
        .await
        .forward::<StageError>("Failed deserializing state node block")?;

    // Find to node (optional)
    let to_node_link = state
        .find_node_link(repository.clone(), to_path.as_str())
        .await
        .unwrap_or_default();

    // Get target file/directory metadata
    let to_absolute_path = to_path.to_absolute_path(repository.require_path()?);
    let to_metadata = tokio::fs::metadata(to_absolute_path)
        .await
        .internal(&format!("Path {to_path} does not exist in repository "))?;

    if from_node.is_directory() && !to_metadata.is_dir() {
        return Err(StageError::internal("Cannot move a directory to a file"));
    }
    if !from_node.is_directory() && to_metadata.is_dir() {
        return Err(StageError::internal("Cannot move a file to a directory"));
    }

    let stats = Arc::new(StageStats::default());

    if to_node_link.is_valid() {
        // Stage existing target node as deleted, it is being replaced by the source file
        lore_debug!(
            "Staging existing target node {} as deleted",
            to_node_link.node
        );
        if to_node_link.repository != repository.id {
            // TODO(vri): UCS-18009 - Implement stage move for linked changes
            return Err(StageError::internal(
                "Links not yet implemented, cannot perform actions in other repositories",
            ));
        }

        stage::stage_delete(
            repository.clone(),
            state.clone(),
            to_node_link.node,
            options.node_flags,
            stats.clone(),
            None, // TODO(vri): UCS-18009 - Implement stage move for linked changes
        )
        .await?;
    }

    // Make sure the target parent node exist
    let mut parent_path = to_path.clone();
    parent_path.pop();
    let parent_absolute_path = parent_path.to_absolute_path(repository.require_path()?);
    lore_debug!(
        "New parent node path: {}/ ({})",
        parent_path,
        parent_absolute_path.display()
    );

    let mut parent_options = options;
    parent_options.no_children = true;

    let parent_node_link = Box::pin(stage::stage_filesystem_path(
        repository.clone(),
        state.clone(),
        repository.require_path()?.to_path_buf(),
        RelativePathBuf::new(),
        ROOT_NODE,
        parent_path,
        stats.clone(),
        parent_options,
        None, // TODO(vri): UCS-18009 - Implement stage move for linked changes
        None,
    ))
    .await?;

    let block_index = NodeBlock::index(from_node_link.node);
    let node_index = Node::index(from_node_link.node);
    let block = state
        .block(repository.clone(), block_index)
        .await
        .forward::<StageError>("Failed deserializing state node block")?;
    let mut node = block.node(node_index);

    if node.parent != parent_node_link.node {
        // Unlink it from the previous parent child list
        lore_debug!(
            "Unlink node {} from previous parent node: {}",
            from_node_link.node,
            node.parent
        );
        let parent_block_index = NodeBlock::index(node.parent);
        let parent_node_index = Node::index(node.parent);
        let parent_block = state
            .block(repository.clone(), parent_block_index)
            .await
            .forward::<StageError>("Failed deserializing state node block")?;
        let parent_node = parent_block.node(parent_node_index);
        if parent_node.child == from_node_link.node {
            lore_debug!(
                "Parent {} child node match, new child node: {}",
                node.parent,
                node.sibling
            );
            let dirtied = {
                let mut block_writer = parent_block.write();
                block_writer.node(parent_node_index).child = node.sibling;
                block_writer.mark_dirty()
            };
            if dirtied {
                state.block_modified(parent_block, parent_block_index);
                state.mark_dirty();
            }
        } else {
            lore_debug!(
                "Parent {} child node does not match, find in sibling list",
                node.parent
            );
            let mut found = false;
            let parent_id = node.parent;
            let mut child_id = parent_node.child().unwrap_or_default();
            let mut cycle = SiblingCycleGuard::new(parent_id);
            while let Some(sibling) = {
                let child = state
                    .node(repository.clone(), child_id)
                    .await
                    .forward::<StageError>("Failed deserializing state node block")?;
                child
                    .walk_step(child_id, parent_id, &mut cycle)
                    .forward::<StageError>("Invalid node hierarchy in stage walk")?;
                child.sibling()
            } {
                if sibling == from_node_link.node {
                    lore_debug!(
                        "Node {} sibling match, replace with new sibling {}",
                        child_id,
                        node.sibling
                    );
                    let child_block_index = NodeBlock::index(child_id);
                    let child_node_index = Node::index(child_id);
                    let child_block =
                        state
                            .block(repository.clone(), child_block_index)
                            .await
                            .forward::<StageError>("Failed deserializing state node block")?;
                    let dirtied = {
                        let mut block_writer = child_block.write();
                        block_writer.node(child_node_index).sibling = node.sibling;
                        block_writer.mark_dirty()
                    };
                    if dirtied {
                        state.block_modified(child_block, child_block_index);
                        state.mark_dirty();
                    }
                    found = true;
                    break;
                }
                lore_debug!(
                    "Node {} sibling does not match, move to {}",
                    child_id,
                    sibling
                );
                child_id = sibling;
            }
            if !found {
                return Err(StageError::internal(
                    "Node not found in child node list, inconsistent repository state",
                ));
            }
        }

        // Inject it into the new parent child list
        lore_debug!(
            "Link node {} to new parent node {} child list",
            from_node_link.node,
            parent_node_link.node
        );
        let parent_block_index = NodeBlock::index(parent_node_link.node);
        let parent_node_index = Node::index(parent_node_link.node);
        let parent_block = state
            .block(repository.clone(), parent_block_index)
            .await
            .forward::<StageError>("Failed deserializing state node block")?;
        let sibling_node_id = parent_block.node(parent_node_index).child;
        let dirtied = {
            let mut block_writer = parent_block.write();
            block_writer.node(parent_node_index).child = from_node_link.node;
            block_writer.mark_dirty()
        };
        if dirtied {
            state.block_modified(parent_block, parent_block_index);
            state.mark_dirty();
        }

        lore_debug!(
            "Update node {} sibling node to {}",
            from_node_link.node,
            sibling_node_id
        );
        node.sibling = sibling_node_id;
    }

    // Set the new node metadata - parent node and name (sibling node set above)
    {
        lore_debug!(
            "Update node {} parent to node {}",
            from_node_link.node,
            parent_node_link.node
        );
        node.parent = parent_node_link.node;

        let from_name = from_path.name();
        let to_name = to_path.name();
        if from_name != to_name {
            // Rename the from node
            block
                .deserialize_nametable(repository.clone())
                .await
                .forward::<StageError>("Failed deserializing name table")?;
            lore_debug!(
                "Rename node {}: {} -> {}",
                from_node_link.node,
                from_name,
                to_name
            );
            node.name_hash = hash_string(to_name);
            (node.name_offset, node.name_length) = block
                .write()
                .node_name_store(to_name, node.name_offset, node.name_length)
                .forward::<StageError>("Storing renamed node name")?;
        }
    }

    let dirtied = {
        let mut block_writer = block.write();
        *block_writer.node(node_index) = node;
        block_writer.mark_dirty()
    };
    if dirtied {
        state.block_modified(block, block_index);
        state.mark_dirty();
    }

    // Mark from node as moved
    let move_flag = if from_node.is_staged_add() {
        NodeFlags::StagedAdd
    } else {
        NodeFlags::StagedMove
    };
    state
        .node_mark(
            repository.clone(),
            from_node_link.node,
            move_flag,
            true, /* Mark dirty */
        )
        .await
        .forward::<StageError>("Failed to mark node as staged")?;

    // If this is a directory move, recursively mark all children as moved
    if from_node.is_directory() {
        mark_children_moved(
            repository.clone(),
            state.clone(),
            from_node_link.node,
            move_flag,
        )
        .await
        .forward::<StageError>("Failed to mark node as staged")?;
    }

    #[allow(clippy::collapsible_else_if)]
    if from_node.is_staged_add() {
        if from_node.is_directory() {
            stats.directory_add_count.fetch_add(1, Ordering::Relaxed);
        } else {
            stats.file_add_count.fetch_add(1, Ordering::Relaxed);
        }
    } else {
        if from_node.is_directory() {
            stats.directory_move_count.fetch_add(1, Ordering::Relaxed);
        } else {
            stats.file_move_count.fetch_add(1, Ordering::Relaxed);
        }
    }

    // TODO(vri): UCS-18009 - Implement stage move for linked changes
    // Serialize all staged links states recursively

    let count = LoreFileStageCountData::new(stats.clone());
    event::LoreEvent::FileStageEnd(LoreFileStageEndEventData { count }).send();

    state.set_parent_self(current_revision);

    // If staged state is the initial stage based on current state, reset other parent. Otherwise
    // leave it as is, in case previous staged state was a merge/integrate
    if state.revision() == current_revision {
        state.set_parent_other(Hash::default());
        state.set_metadata_hash(Hash::default());
    }

    // Serialize new staged state
    let signature = state
        .serialize(repository.clone(), token)
        .await
        .forward::<StageError>("Failed to serialize staged revision state")?;
    crate::instance::store_staged_anchor(&repository, signature)
        .await
        .forward::<StageError>("Failed to serialize staged anchor")?;

    event::LoreEvent::FileStageRevision(LoreFileStageRevisionEventData {
        repository: repository.id,
        revision: signature,
    })
    .send();

    Ok(signature)
}

/// Routing decision for a single stage path against the configured layer set.
///
/// `Inside` routes exclusively to one layer with a possibly-empty `remain` suffix.
/// `AncestorOf` routes to the parent (with the listed layer subtrees masked) AND
/// to every layer whose `target_path` is under the input path. `Disjoint` routes
/// to the parent only.
///
/// Layer indices refer into the slice passed to [`classify_stage_path`].
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum LayerRoute {
    Inside { layer_index: usize, remain: String },
    AncestorOf { layer_indices: Vec<usize> },
    Disjoint,
}

/// Classifies a stage path against a list of layer mount paths (`target_path`s).
///
/// Assumes non-overlapping layers (no layer's `target_path` is a prefix of another's).
pub(crate) fn classify_stage_path(relative_path: &str, layer_target_paths: &[&str]) -> LayerRoute {
    if relative_path.is_empty() {
        return if layer_target_paths.is_empty() {
            LayerRoute::Disjoint
        } else {
            LayerRoute::AncestorOf {
                layer_indices: (0..layer_target_paths.len()).collect(),
            }
        };
    }

    for (i, target) in layer_target_paths.iter().enumerate() {
        if target.is_empty() {
            continue;
        }
        if relative_path == *target {
            return LayerRoute::Inside {
                layer_index: i,
                remain: String::new(),
            };
        }
        if let Some(rest) = relative_path.strip_prefix(target)
            && rest.starts_with('/')
        {
            return LayerRoute::Inside {
                layer_index: i,
                remain: rest[1..].to_string(),
            };
        }
    }

    let mut ancestor_indices = Vec::new();
    for (i, target) in layer_target_paths.iter().enumerate() {
        if target.is_empty() {
            continue;
        }
        if let Some(rest) = target.strip_prefix(relative_path)
            && rest.starts_with('/')
        {
            ancestor_indices.push(i);
        }
    }

    if ancestor_indices.is_empty() {
        LayerRoute::Disjoint
    } else {
        LayerRoute::AncestorOf {
            layer_indices: ancestor_indices,
        }
    }
}

/// Returns true if `relative_path` is at or inside any of the masked subtree paths.
///
/// Used by the parent stage walker to skip layer mount subtrees so files inside
/// layers aren't double-counted on the parent side.
///
/// Generic over `AsRef<str>` so callers can pass either `&[String]`
/// (production: layer target paths) or `&[&str]` (tests). This avoids the
/// per-call Vec<&str> rebuild that the previous `&[&str]`-only signature
/// forced on the production hot path.
pub(crate) fn is_path_under_layer_mask<S: AsRef<str>>(relative_path: &str, mask: &[S]) -> bool {
    for entry in mask {
        let entry = entry.as_ref();
        if entry.is_empty() {
            continue;
        }
        if relative_path == entry {
            return true;
        }
        if let Some(rest) = relative_path.strip_prefix(entry)
            && rest.starts_with('/')
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod mask_tests {
    use super::*;

    #[test]
    fn empty_mask_never_masks() {
        let empty: [&str; 0] = [];
        assert!(!is_path_under_layer_mask("external/lib", &empty));
        assert!(!is_path_under_layer_mask("", &empty));
    }

    #[test]
    fn exact_mask_match_is_masked() {
        assert!(is_path_under_layer_mask("external/lib", &["external/lib"]));
    }

    #[test]
    fn path_inside_masked_subtree_is_masked() {
        assert!(is_path_under_layer_mask(
            "external/lib/src/foo.rs",
            &["external/lib"]
        ));
    }

    #[test]
    fn ancestor_of_masked_path_is_not_masked() {
        // Walker entering "external" should still descend; the mask kicks in
        // when it reaches "external/lib".
        assert!(!is_path_under_layer_mask("external", &["external/lib"]));
    }

    #[test]
    fn disjoint_path_is_not_masked() {
        assert!(!is_path_under_layer_mask("src/main.rs", &["external/lib"]));
    }

    #[test]
    fn empty_path_with_mask_is_not_masked() {
        // The parent's root is never itself masked.
        assert!(!is_path_under_layer_mask("", &["external/lib"]));
    }

    #[test]
    fn prefix_string_match_without_separator_is_not_masked() {
        assert!(!is_path_under_layer_mask(
            "external_other/file.rs",
            &["external"]
        ));
    }

    #[test]
    fn multiple_mask_entries_any_match_is_masked() {
        let mask = ["external/lib", "vendor/foo"];
        assert!(is_path_under_layer_mask("vendor/foo/x.rs", &mask));
        assert!(is_path_under_layer_mask("external/lib", &mask));
        assert!(!is_path_under_layer_mask("src/main.rs", &mask));
    }
}

#[cfg(test)]
mod classify_tests {
    use super::*;

    #[test]
    fn empty_path_no_layers_is_disjoint() {
        assert_eq!(classify_stage_path("", &[]), LayerRoute::Disjoint);
    }

    #[test]
    fn empty_path_with_layers_is_ancestor_of_all() {
        let layers = ["external/lib", "vendor/foo"];
        assert_eq!(
            classify_stage_path("", &layers),
            LayerRoute::AncestorOf {
                layer_indices: vec![0, 1],
            }
        );
    }

    #[test]
    fn exact_layer_match_is_inside_with_empty_remain() {
        let layers = ["external/lib"];
        assert_eq!(
            classify_stage_path("external/lib", &layers),
            LayerRoute::Inside {
                layer_index: 0,
                remain: String::new(),
            }
        );
    }

    #[test]
    fn path_inside_layer_is_inside_with_remain() {
        let layers = ["external/lib"];
        assert_eq!(
            classify_stage_path("external/lib/src/foo.rs", &layers),
            LayerRoute::Inside {
                layer_index: 0,
                remain: "src/foo.rs".into(),
            }
        );
    }

    #[test]
    fn path_ancestor_of_one_layer_is_ancestor_of_that_layer() {
        let layers = ["external/lib", "src/main.rs"];
        assert_eq!(
            classify_stage_path("external", &layers),
            LayerRoute::AncestorOf {
                layer_indices: vec![0],
            }
        );
    }

    #[test]
    fn path_ancestor_of_multiple_layers_lists_them_all() {
        let layers = ["vendor/a", "vendor/b", "external/lib"];
        assert_eq!(
            classify_stage_path("vendor", &layers),
            LayerRoute::AncestorOf {
                layer_indices: vec![0, 1],
            }
        );
    }

    #[test]
    fn disjoint_path_with_layers_is_disjoint() {
        let layers = ["external/lib", "vendor/foo"];
        assert_eq!(
            classify_stage_path("src/main.rs", &layers),
            LayerRoute::Disjoint
        );
    }

    #[test]
    fn prefix_string_match_without_separator_is_disjoint_not_inside() {
        // "external" is a string prefix of "external_other" but not a path-prefix.
        // Confirms we check '/' boundary, not bare string prefix.
        let layers = ["external"];
        assert_eq!(
            classify_stage_path("external_other", &layers),
            LayerRoute::Disjoint
        );
    }
}
