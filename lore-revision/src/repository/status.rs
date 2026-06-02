// SPDX-FileCopyrightText: 2026 Epic Games, Inc.
// SPDX-License-Identifier: MIT
#[cfg(target_family = "unix")]
use std::os::unix::fs::MetadataExt;
#[cfg(target_family = "windows")]
use std::os::windows::fs::MetadataExt;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use lore_base::lore_spawn;
use lore_error_set::prelude::*;
use serde::Deserialize;
use serde::Serialize;
use tokio::task::JoinSet;

use super::RepositoryContext;
use crate::branch;
use crate::change::FileAction;
use crate::change::NodeChange;
use crate::errors::*;
use crate::event;
use crate::event::EventError;
use crate::filter::FilterMode;
use crate::find;
use crate::interface::LoreError;
use crate::interface::LoreFileAction;
use crate::interface::LoreNodeType;
use crate::interface::LoreString;
use crate::layer;
use crate::lore::BranchId;
use crate::lore::Hash;
use crate::lore::RepositoryId;
use crate::lore_debug;
use crate::lore_drain_tasks;
use crate::lore_trace;
use crate::metadata::Metadata;
use crate::node::NodeFlags;
use crate::node::NodeIDExt;
use crate::path::emit_path_ignore;
use crate::state;
use crate::util::path::RelativePath;
use crate::util::serde::u8_as_bool;

#[repr(C)]
#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LoreRepositoryStatusRevisionEventData {
    pub repository: RepositoryId,
    pub branch: BranchId,
    pub branch_name: LoreString,
    pub revision: Hash,
    pub revision_number: u64,
    pub revision_staged: Hash,
    pub revision_merged: Hash,
    pub revision_merged_parent_branch: Hash,
    pub revision_local: Hash,
    pub revision_local_number: u64,
    pub revision_remote: Hash,
    pub revision_remote_number: u64,
    pub is_local_ahead: u8,
    pub is_remote_ahead: u8,
    pub remote_available: u8,
    pub remote_branch_exist: u8,
}

#[allow(clippy::too_many_arguments)]
#[allow(clippy::fn_params_excessive_bools)]
impl LoreRepositoryStatusRevisionEventData {
    pub fn new(
        repository: RepositoryId,
        branch: BranchId,
        branch_name: &str,
        revision: Hash,
        revision_number: u64,
        revision_staged: Hash,
        revision_merged: Hash,
        revision_merged_parent_branch: Hash,
        revision_local: Hash,
        revision_local_number: u64,
        revision_remote: Hash,
        revision_remote_number: u64,
        is_local_ahead: bool,
        is_remote_ahead: bool,
        remote_available: bool,
        remote_branch_exist: bool,
    ) -> Self {
        LoreRepositoryStatusRevisionEventData {
            repository,
            branch,
            branch_name: branch_name.into(),
            revision,
            revision_number,
            revision_staged,
            revision_merged,
            revision_merged_parent_branch,
            revision_local,
            revision_local_number,
            revision_remote,
            revision_remote_number,
            is_local_ahead: is_local_ahead.into(),
            is_remote_ahead: is_remote_ahead.into(),
            remote_available: remote_available.into(),
            remote_branch_exist: remote_branch_exist.into(),
        }
    }
}

#[repr(C)]
#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LoreRepositoryStatusFileEventData {
    pub path: LoreString,
    pub size: u64,
    pub action: LoreFileAction,
    pub r#type: LoreNodeType,

    #[serde(with = "u8_as_bool")]
    pub flag_staged: u8,
    #[serde(with = "u8_as_bool")]
    pub flag_merged: u8,
    #[serde(with = "u8_as_bool")]
    pub flag_conflict: u8,
    #[serde(with = "u8_as_bool")]
    pub flag_conflict_unresolved: u8,
    #[serde(with = "u8_as_bool")]
    pub flag_conflict_automerged: u8,
    #[serde(with = "u8_as_bool")]
    pub flag_conflict_mine: u8,
    #[serde(with = "u8_as_bool")]
    pub flag_conflict_theirs: u8,
    #[serde(with = "u8_as_bool")]
    pub flag_dirty: u8,

    pub from_path: LoreString,
}

impl LoreRepositoryStatusFileEventData {
    pub fn from_node_change(change: &NodeChange, size: u64) -> Self {
        let node_type = if change.action == FileAction::Add
            || change.action == FileAction::Move
            || change.to.node.is_valid_node_id()
        {
            change.to.flags
        } else {
            change.from.flags
        };
        let node_type = if node_type.contains(NodeFlags::File) {
            LoreNodeType::File
        } else if node_type.contains(NodeFlags::Link) {
            LoreNodeType::Link
        } else {
            LoreNodeType::Directory
        };
        LoreRepositoryStatusFileEventData {
            path: LoreString::from(&change.path),
            size,
            action: LoreFileAction::from(change.action),
            r#type: node_type,
            flag_dirty: change.flags.is_dirty().into(),
            flag_staged: change.flags.is_stage().into(),
            flag_merged: change.flags.is_merge().into(),
            flag_conflict: change.flags.is_conflict().into(),
            flag_conflict_unresolved: change.flags.is_conflict_unresolved().into(),
            flag_conflict_automerged: change.flags.is_conflict_automerged().into(),
            flag_conflict_mine: change.flags.is_conflict_mine().into(),
            flag_conflict_theirs: change.flags.is_conflict_theirs().into(),
            from_path: change.from_path.as_ref().map(|path| path.as_str()).into(),
        }
    }

    pub fn action_as_string_short(&self) -> &'static str {
        self.action.as_string_short()
    }

    pub fn merged_as_string_short(&self) -> &'static str {
        if self.flag_merged != 0 {
            return "(M)";
        }
        ""
    }

    pub fn conflict_as_string_short(&self) -> &'static str {
        if self.flag_conflict != 0 && self.flag_conflict_unresolved != 0 {
            return "!";
        }
        ""
    }
}

#[error_set]
pub enum StatusError {
    NodeNotFound,
    LinkNotFound,
    NotFound,
    FileNotFound,
    RevisionNotFound,
    WriteRequired,
    Oversized,
    InvalidArguments,
    InvalidPath,
    InvalidNodeHierarchy,
    AddressNotFound,
    PayloadNotFound,
    AlreadyLinked,
    LayerNotFound,
    Disconnected,
    SlowDown,
    NotAuthorized,
    NotAuthenticated,
    Maintenance,
    NoRemote,
    NotSupported,
    BranchAdvanced,
    BranchAlreadyExists,
    BranchNotFound,
    Conflict,
    DeleteCurrent,
    DeleteDefault,
    DeleteProtected,
    Divergent,
    IdenticalMetadata,
    LinkPathNotFound,
    LocalModifications,
    LockNotFound,
    LockNotOwned,
    MaxHistorySearchDepth,
    NotALayer,
    NotALink,
    NotConnected,
    NothingStaged,
    RepositoryAlreadyExists,
    RepositoryNotFound,
    SharedStoreNotFound,
    TokenNotFound,
    MissingIdentity,
}

impl EventError for StatusError {
    fn translated(&self) -> LoreError {
        match self {
            StatusError::Disconnected(_) => LoreError::Connection,
            StatusError::SlowDown(_) => LoreError::SlowDown,
            StatusError::Oversized(_) => LoreError::Oversized,
            StatusError::FileNotFound(_) => LoreError::FileNotFound,
            StatusError::NotFound(_)
            | StatusError::LayerNotFound(_)
            | StatusError::RevisionNotFound(_) => LoreError::NotFound,
            StatusError::AddressNotFound(_) => LoreError::AddressNotFound,
            StatusError::PayloadNotFound(_) => LoreError::PayloadNotFound,
            StatusError::InvalidArguments(_) | StatusError::InvalidPath(_) => {
                LoreError::InvalidArguments
            }
            _ => LoreError::Internal,
        }
    }

    fn inner(&self) -> String {
        self.to_string()
    }
}

#[derive(Clone, Debug)]
pub struct StatusOptions {
    // Include staged or not
    pub staged: bool,
    /// Reconcile against the filesystem and refresh dirty tracking.
    ///
    /// When `false` (default), status reports the currently tracked state:
    /// the staged revision (if any) plus all files and directories marked
    /// dirty in the repository. No filesystem reads are performed beyond the
    /// existing dirty flags.
    ///
    /// When `true`, the filesystem is walked under each requested path,
    /// every file is reconciled against the current revision, and dirty
    /// flags are set or cleared accordingly. The refreshed flags are
    /// persisted in the staged state so subsequent operations see an
    /// accurate picture without rescanning.
    pub scan: bool,
    // Drop the existing staged anchor before computing status.
    // Combine with `scan` to scan from a clean slate.
    pub reset: bool,
    // Include sync point
    pub sync_point: bool,
    // Only emit revision info, skip all diffs
    pub revision_only: bool,
}

async fn file_size_from_node_change_id(change: &NodeChange) -> Result<u64, StatusError> {
    if change.action == FileAction::Delete {
        Ok(0)
    } else {
        let size = change
            .to
            .state
            .node(change.to.repository.clone(), change.to.node)
            .await
            .forward::<StatusError>("accessing node path")?
            .size;
        Ok(size)
    }
}

async fn file_size_from_node_change_path(
    repository_path: &Path,
    change: &NodeChange,
) -> Result<u64, StatusError> {
    if change.action == FileAction::Delete {
        Ok(0)
    } else {
        let path_str = change.path.as_str().to_string();
        let metadata = tokio::fs::metadata(change.path.to_absolute_path(repository_path))
            .await
            .internal_with(|| format!("accessing metadata for file {path_str}"))?;
        #[cfg(target_family = "windows")]
        let size = metadata.file_size();
        #[cfg(target_family = "unix")]
        let size = metadata.size();
        Ok(size)
    }
}

pub async fn status(
    repository: Arc<RepositoryContext>,
    paths: Option<Vec<RelativePath>>,
    options: StatusOptions,
) -> Result<(), StatusError> {
    if options.reset {
        crate::instance::delete_staged_anchor(&repository)
            .await
            .forward::<StatusError>("dropping staged anchor for status reset")?;
    }

    let (state_current, state_staged, current_branch) =
        state::State::deserialize_current_and_staged(repository.clone())
            .await
            .forward::<StatusError>("deserializing current and staged state")?;

    let mut has_staged = state_staged.is_some();
    let state_staged = state_staged.unwrap_or_else(|| state_current.clone());

    lore_debug!(
        "Repository status, current signature {}, staged signature {}",
        state_current.revision(),
        state_staged.revision()
    );

    let layers = {
        let mut layers = vec![];
        let list = layer::list(repository.clone()).await.unwrap_or_default();
        for layer in list {
            let layer_state = layer
                .deserialize_current_and_staged(repository.clone())
                .await
                .forward::<StatusError>("deserializing layer state")?;

            if !layer_state.state_staged.revision().is_zero()
                && layer_state.state_staged.revision() != layer_state.state_current.revision()
            {
                has_staged = true;
            }

            layers.push((layer, layer_state));
        }
        layers
    };

    // Pre-resolve layer mount metadata for the parent's filesystem walker:
    // for each configured layer, find the source_path node in the layer's
    // staged state. When the walker hits one of these mount paths it switches
    // comparison context to the layer's tree rather than treating the
    // mount-point contents as parent-tree adds.
    let layer_mounts: Arc<Vec<state::LayerMountInfo>> = {
        let mut mounts = Vec::new();
        for (layer, layer_state) in layers.iter() {
            let source_node_link = layer_state
                .state_staged
                .find_node_link(layer_state.repository.clone(), &layer.source_path)
                .await;
            let Ok(source_node_link) = source_node_link else {
                lore_debug!(
                    "Skipping layer mount {} — source path {} not found in layer state",
                    layer.target_path,
                    layer.source_path
                );
                continue;
            };
            mounts.push(state::LayerMountInfo {
                target_path: layer.target_path.clone(),
                repository: layer_state.repository.clone(),
                state: layer_state.state_staged.clone(),
                source_node: source_node_link.node,
            });
        }
        Arc::new(mounts)
    };

    let branch_metadata = branch::metadata(repository.clone(), current_branch)
        .await
        .forward::<StatusError>("loading branch metadata")?;
    let branch = branch::branch_metadata(repository.clone(), current_branch, &branch_metadata)
        .await
        .forward::<StatusError>("loading branch info")?;
    let branch_stack = branch::stack(&branch_metadata);

    let show_staged = options.staged;
    let show_scan = options.scan;

    let local_latest = branch::load_latest(repository.clone(), branch.id)
        .await
        .unwrap_or_default();

    let local_state = state::State::deserialize(repository.clone(), local_latest)
        .await
        .forward::<StatusError>("deserializing local state")?;

    let remote_latest = if let Ok(remote) = repository.remote().await {
        branch::load_remote_latest(remote.clone(), repository.id, branch.id)
            .await
            .ok()
    } else {
        None
    };

    let remote_state = if let Some(remote_latest) = remote_latest {
        state::State::deserialize(repository.clone(), remote_latest)
            .await
            .ok()
    } else {
        None
    };

    let branch_parent = branch_stack
        .first()
        .map(|parent| parent.branch)
        .unwrap_or_default();
    let branch_point = branch_stack
        .first()
        .map(|parent| parent.revision)
        .unwrap_or_default();

    let revision_merged_parent_branch = if options.sync_point {
        if branch_point.is_zero() {
            Hash::default()
        } else {
            let mut search_point = state_current.revision();

            // Repeatedly search for a revision that's the result of a merge and then
            // check if the merged revision was coming from the parent branch.
            loop {
                let Ok(signature) = find::find_revision(
                    repository.clone(),
                    current_branch,
                    search_point,
                    false,
                    None,
                    |state, _metadata| {
                        let is_branch_point = state.revision() == branch_point;
                        let is_merge = !state.parent_other().is_zero();

                        if is_merge || is_branch_point {
                            find::FindMatchResult::Match
                        } else {
                            find::FindMatchResult::Continue
                        }
                    },
                )
                .await
                else {
                    break Hash::default();
                };

                if signature == branch_point {
                    lore_debug!(
                        "Found branch point {} as last merged in from parent branch",
                        signature
                    );
                    break signature;
                }

                let branch_state = state::State::deserialize(repository.clone(), signature)
                    .await
                    .forward::<StatusError>("deserializing branch state")?;
                let parent_state =
                    state::State::deserialize(repository.clone(), branch_state.parent_other())
                        .await
                        .forward::<StatusError>("deserializing parent state")?;
                let parent_state_metadata =
                    Metadata::deserialize(repository.clone(), parent_state.metadata_hash())
                        .await
                        .forward::<StatusError>("deserializing parent metadata")?;
                let parent_state_branch = parent_state_metadata
                    .get_branch()
                    .forward::<StatusError>("reading parent branch from metadata")?;
                if parent_state_branch == branch_parent {
                    lore_debug!(
                        "Found revision {} as last merged in from parent branch",
                        parent_state.revision()
                    );
                    break parent_state.revision();
                }

                search_point = branch_state.parent_self();
            }
        }
    } else {
        Hash::default()
    };

    let mut local_ahead = false;
    let mut remote_ahead = false;

    let last_sync = branch::load_last_sync(repository.clone(), branch.id)
        .await
        .unwrap_or_default();

    // Authoritative answer to "does local have commits not on remote history?":
    // the LATEST_STATUS flag set by commit/push/sync/clone/restore. When
    // Convergent, local_latest is guaranteed to be on the remote history line —
    // any difference can only mean remote moved past us.
    let local_diverged = branch::load_latest_divergent(repository.clone(), branch.id)
        .await
        .unwrap_or(true);

    if local_latest != remote_latest.unwrap_or_default()
        && let Some(remote_state) = remote_state.clone()
    {
        let local_n = local_state.revision_number();
        let remote_n = remote_state.revision_number();
        if !local_diverged {
            remote_ahead = remote_n > local_n;
        } else if remote_n > local_n {
            // Local has unpushed work AND remote moved beyond it.
            local_ahead = true;
            remote_ahead = true;
        } else if local_n > remote_n {
            local_ahead = true;
            // Refine with last_sync: if remote has moved beyond the last
            // recorded sync point, it has commits we don't have.
            if last_sync != remote_latest.unwrap_or_default() {
                remote_ahead = true;
            }
        } else {
            // Same revision number, different hashes — divergent.
            local_ahead = true;
            remote_ahead = true;
        }
    }
    {
        let status = match (remote_ahead, local_ahead) {
            (true, true) => "divergent",
            (true, false) => "remote ahead",
            (false, true) => "local ahead",
            (false, false) => "synchronized",
        };
        lore_debug!(
            "Branch is {}, remote LATEST {}, local LATEST {}, last sync {}",
            status,
            remote_latest.unwrap_or_default(),
            local_latest,
            last_sync
        );
    }

    {
        let data = LoreRepositoryStatusRevisionEventData::new(
            repository.id,
            branch.id,
            branch.name.as_str(),
            state_current.revision(),
            state_current.revision_number(),
            if has_staged {
                state_staged.revision()
            } else {
                Hash::default()
            },
            state_staged.parent_other(),
            revision_merged_parent_branch,
            local_state.revision(),
            local_state.revision_number(),
            remote_latest.unwrap_or_default(),
            if let Some(remote_state) = remote_state {
                remote_state.revision_number()
            } else {
                0
            },
            local_ahead,
            remote_ahead,
            repository.remote().await.is_ok(),
            remote_latest.is_some(),
        );
        lore_debug!("Repository status: {data:?}");
        event::LoreEvent::RepositoryStatusRevision(data).send();
    }

    if options.revision_only {
        return Ok(());
    }

    let paths = match paths.map(RelativePath::dedup_to_supersets) {
        // Caller supplied a path filter that survived dedup — iterate it.
        Some(deduped) if !deduped.is_empty() => deduped.into_iter().map(Some).collect(),
        // No filter, or dedup collapsed to the repository root — scan everything.
        _ => vec![None],
    };

    // Compare current state against staged state
    if show_staged && has_staged {
        lore_debug!("Calculating deltas against staged revision");

        let mut tasks = JoinSet::new();
        for path in paths.iter() {
            lore_spawn!(tasks, {
                let repository = repository.clone();
                let state_current = state_current.clone();
                let state_staged = state_staged.clone();
                let path = path.clone();
                async move {
                    let changes = state::diff_collect(
                        repository.clone(),
                        state_current,
                        repository.clone(),
                        state_staged.clone(),
                        path,
                        FilterMode::Full,
                    )
                    .await
                    .forward::<StatusError>("computing diff against staged state")?;
                    lore_debug!("Found {} changes in staged revision", changes.len());

                    for change in changes.iter() {
                        // When scanning, skip dirty-only changes from the
                        // state diff — the scan section re-detects them from
                        // the filesystem and handles set/clear inline.
                        let dominated_by_scan =
                            show_scan && change.flags.is_dirty() && !change.flags.is_stage();
                        if !dominated_by_scan
                            && (change.flags.is_stage() || change.flags.is_dirty())
                        {
                            let size = file_size_from_node_change_id(change).await?;

                            event::LoreEvent::RepositoryStatusFile(
                                LoreRepositoryStatusFileEventData::from_node_change(change, size),
                            )
                            .send();
                        }
                    }

                    Ok(())
                }
            });

            for (layer, layer_state) in layers.iter() {
                let target_path =
                    RelativePath::new_from_initial_path(&layer.target_path).unwrap_or_default();
                let path = path.clone().unwrap_or_else(|| target_path.clone());
                if path.is_empty() || path.overlaps(&layer.target_path) {
                    lore_spawn!(tasks, {
                        let repository = layer_state.repository.clone();
                        let state_current = layer_state.state_current.clone();
                        let state_staged = layer_state.state_staged.clone();
                        let source_path = layer.source_path.clone();
                        let sub_path = if path.as_str().len() > target_path.len() {
                            &path.as_str()[target_path.len()..]
                        } else {
                            ""
                        };
                        let path = RelativePath::new_from_clean_parts(&source_path, sub_path);
                        let path = if !path.is_empty() { Some(path) } else { None };
                        async move {
                            let mut changes = state::diff_collect(
                                repository.clone(),
                                state_current,
                                repository.clone(),
                                state_staged.clone(),
                                path,
                                FilterMode::Full,
                            )
                            .await
                            .forward::<StatusError>("computing diff against staged state")?;
                            lore_debug!(
                                "Found {} changes in layer \"{}\" staged revision",
                                target_path,
                                changes.len()
                            );

                            for change in changes.iter_mut() {
                                // TODO(mjansson): Translate paths for file size
                                let size = 0;
                                /*
                                let size = file_size_from_node_change_id(change).await?;
                                */

                                change
                                    .translate_from_layer_path(&source_path, target_path.as_str());

                                event::LoreEvent::RepositoryStatusFile(
                                    LoreRepositoryStatusFileEventData::from_node_change(
                                        change, size,
                                    ),
                                )
                                .send();
                            }

                            Ok(())
                        }
                    });
                }
            }
        }

        lore_drain_tasks!(tasks, StatusError::internal("Recursion task failed"))?;
    }

    // Compare current/staged state against filesystem
    if show_scan {
        lore_debug!(
            "Calculating deltas against filesystem for {} paths",
            paths.len()
        );

        let mut tasks = JoinSet::new();
        for path in paths.iter() {
            let repository = repository.clone();
            let state_current = state_current.clone();
            let state_staged = state_staged.clone();
            let path = path.clone();
            let layer_mounts = layer_mounts.clone();
            let exists = if let Some(path) = path.as_ref() {
                let mut exists_in_state = false;
                let mut exists_in_filesystem = false;

                let state = if has_staged {
                    state_staged.clone()
                } else {
                    state_current.clone()
                };

                let node_link = state
                    .find_node_link(repository.clone(), path.as_str())
                    .await
                    .unwrap_or_default();
                if node_link.is_valid() {
                    exists_in_state = true;
                } else {
                    let absolute_path = path.to_absolute_path(repository.require_path()?);
                    exists_in_filesystem = std::fs::exists(absolute_path).unwrap_or_default();
                }

                if !exists_in_state && !exists_in_filesystem {
                    emit_path_ignore(path.as_str()).await;
                    lore_trace!("Ignoring invalid path: {path}");
                }

                exists_in_state || exists_in_filesystem
            } else {
                true
            };

            if exists {
                lore_spawn!(tasks, {
                    async move {
                        if let Some(path) = path.as_ref() {
                            lore_debug!(
                                "Calculating deltas against filesystem path: {}",
                                path.as_str()
                            );
                        } else {
                            lore_debug!(
                                "Calculating deltas against filesystem for full repository"
                            );
                        }

                        let start = Instant::now();

                        // Scan uses staged state as diff base with scan_dirty=true.
                        // Content hashes in staged state are either zero (add nodes)
                        // or equal to current revision hashes, so the comparison is
                        // effectively filesystem vs committed content.
                        // The current revision is passed as the second pair so the
                        // walk can distinguish "node exists in staged but not in
                        // committed" — i.e. unstaged adds — from regular tracked
                        // files. Dirty flags are set/cleared inline during the walk.
                        let (changes, _stats) = state::diff_filesystem_ex(
                            repository.clone(),
                            state_staged.clone(),
                            repository.clone(),
                            state_current.clone(),
                            path,
                            FilterMode::Full,
                            true, // scan_dirty
                            layer_mounts.clone(),
                        )
                        .await
                        .forward::<StatusError>("computing diff against filesystem")?;

                        lore_debug!(
                            "Scan found {} file system changes in {:.3}s",
                            changes.len(),
                            start.elapsed().as_secs_f64(),
                        );

                        for change in changes.iter() {
                            let size =
                                file_size_from_node_change_path(repository.require_path()?, change)
                                    .await?;

                            // Emit event for display (dirty set/clear handled inline by diff)
                            if !change.flags.is_stage() {
                                event::LoreEvent::RepositoryStatusFile(
                                    LoreRepositoryStatusFileEventData::from_node_change(
                                        change, size,
                                    ),
                                )
                                .send();
                            } else {
                                lore_debug!("Ignore staged file {}", change.path);
                            }
                        }

                        Ok(())
                    }
                });
            }

            lore_drain_tasks!(tasks, StatusError::internal("Recursion task failed"))?;
        }
    }

    // If the staged state was updated (by scan or other operations), serialize it.
    // When scanning, the state may have been modified even if no staged anchor existed before.
    // Opportunistically serialize only when the context carries write capability.
    // Read-only status invocations leave the dirty state for the next write command to flush.
    if (has_staged || show_scan)
        && state_staged.is_dirty()
        && let Some(token) = repository.try_write_token()
    {
        // Set up staged state metadata if this is a fresh state (cloned from current)
        if !has_staged {
            let current_revision = state_current.revision();
            state_staged.set_revision_number(0);
            state_staged.set_parent_self(current_revision);
            state_staged.set_parent_other(Hash::default());
            state_staged.set_metadata_hash(Hash::default());
        }
        // Serialize the new staged state
        let signature = state_staged
            .serialize(repository.clone(), token)
            .await
            .forward::<StatusError>("serializing staged revision state")?;

        // Serialize the new staged anchor
        crate::instance::store_staged_anchor(&repository, signature)
            .await
            .forward::<StatusError>("serializing staged revision anchor")?;
    }

    Ok(())
}
