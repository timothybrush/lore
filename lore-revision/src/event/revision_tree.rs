// SPDX-FileCopyrightText: 2026 Epic Games, Inc.
// SPDX-License-Identifier: MIT
//! Per-call event data for the low-level memory-based revision control API.
//!
//! Each verb in the `lore_revision_tree_*` namespace terminates a call with
//! one of these events. Successful reads (`resolve_path`, `list_children`,
//! `node_info`, `node_path`, `metadata_get`) carry the result on the event
//! payload; writes (`add`, `delete`, `modify`, `move`, `metadata_set`,
//! `commit`, `close`) carry an outcome discriminator. All structs are
//! `#[repr(C)]` PODs that cbindgen emits into the public capi header.

use lore_base::types::Address;
use lore_base::types::Context;
use lore_base::types::Hash;
use serde::Deserialize;
use serde::Serialize;

use crate::event::LoreErrorCode;
use crate::interface::LoreMetadata;
use crate::interface::LoreString;
use crate::node::NodeID;

/// Delivered on successful `lore_revision_tree_load`. Carries the registry
/// id the caller must pass to subsequent verbs against this revision tree.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoreRevisionTreeLoadedEventData {
    /// Registry id for the loaded revision tree.
    pub handle_id: u64,
}

/// Terminal per-call event for `resolve_path`. On success `error_code ==
/// None` and `node_id` is the resolved node; on failure `node_id` is
/// undefined and `error_code` is populated.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoreRevisionTreeResolvePathCompleteEventData {
    /// Correlation id of the originating call.
    pub id: u64,
    /// The resolved node.
    pub node_id: NodeID,
    /// The outcome of the call.
    pub error_code: LoreErrorCode,
}

/// Per-child event from `list_children`. One event is emitted per entry;
/// the caller correlates entries by `id` and detects end-of-list via the
/// trailing `Complete` event.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoreRevisionTreeChildEventData {
    /// Correlation id of the originating call.
    pub id: u64,
    /// The child node.
    pub node_id: NodeID,
    /// The name of the child node.
    pub name: LoreString,
    /// The parent node.
    pub parent_id: NodeID,
    /// The kind of node.
    pub kind: u32,
    /// The file mode bits.
    pub mode: u16,
    /// The size of the node's content in bytes.
    pub size: u64,
    /// The address of the node's content.
    pub address: Address,
    /// The outcome of the call.
    pub error_code: LoreErrorCode,
}

/// Root-only metadata accompanying `LoreRevisionTreeNodeInfoEventData` when
/// the queried node is the revision root.
///
/// `is_root` is `1` when the inline fields carry data sourced from the
/// Metadata fragment (parent revision signatures, creation timestamp,
/// author identity, metadata key count); `0` for non-root nodes, in which
/// case the inline fields are zero/default. Keeping the discriminator
/// inline rather than wrapping in `Option<_>` keeps the struct
/// `#[repr(C)]`-stable for cbindgen.
#[repr(C)]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoreRevisionTreeRootInfoData {
    /// 1 when the inline fields carry root data; 0 otherwise.
    pub is_root: u8,
    /// The parent revision signatures.
    pub parent: [Hash; 2],
    /// The time the revision was created.
    pub creation_timestamp: i64,
    /// The identity of the revision's author.
    pub author_identity: LoreString,
    /// The number of metadata keys on the revision.
    pub metadata_key_count: u32,
}

/// Terminal per-call event for `node_info`. Carries the same per-node
/// record as `list_children` plus the preserved `file_id` (the
/// `address.context` slot of the node's original add) and, when the
/// queried node is the root, the Metadata-fragment-derived `root_info`.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoreRevisionTreeNodeInfoEventData {
    /// Correlation id of the originating call.
    pub id: u64,
    /// The queried node.
    pub node_id: NodeID,
    /// The name of the node.
    pub name: LoreString,
    /// The parent node.
    pub parent_id: NodeID,
    /// The kind of node.
    pub kind: u32,
    /// The file mode bits.
    pub mode: u16,
    /// The size of the node's content in bytes.
    pub size: u64,
    /// The address of the node's content.
    pub address: Address,
    /// The preserved file id of the node.
    pub file_id: Context,
    /// Root metadata, valid only when the node is the revision root.
    pub root_info: LoreRevisionTreeRootInfoData,
}

/// Terminal per-call event for `node_path`. On success `path` is the
/// reconstructed UTF-8 path from the root to the queried node; on failure
/// `path` is empty and `error_code` is populated.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoreRevisionTreeNodePathEventData {
    /// Correlation id of the originating call.
    pub id: u64,
    /// The reconstructed path from the root to the queried node.
    pub path: LoreString,
    /// The outcome of the call.
    pub error_code: LoreErrorCode,
}

/// Terminal per-call event for `add`. On success `node_id` is the
/// newly-allocated child; on failure `node_id` is undefined.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoreRevisionTreeAddCompleteEventData {
    /// Correlation id of the originating call.
    pub id: u64,
    /// The newly-added node.
    pub node_id: NodeID,
    /// The outcome of the call.
    pub error_code: LoreErrorCode,
}

/// Terminal per-call event for `delete`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoreRevisionTreeDeleteCompleteEventData {
    /// Correlation id of the originating call.
    pub id: u64,
    /// The outcome of the call.
    pub error_code: LoreErrorCode,
}

/// Terminal per-call event for `modify`. `node_id` echoes the modified
/// node so the caller can chain operations without re-resolving.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoreRevisionTreeModifyCompleteEventData {
    /// Correlation id of the originating call.
    pub id: u64,
    /// The modified node.
    pub node_id: NodeID,
    /// The outcome of the call.
    pub error_code: LoreErrorCode,
}

/// Terminal per-call event for `move`. `node_id` echoes the moved node so
/// the caller observes that `file_id` is preserved across the reparent.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoreRevisionTreeMoveCompleteEventData {
    /// Correlation id of the originating call.
    pub id: u64,
    /// The moved node.
    pub node_id: NodeID,
    /// The outcome of the call.
    pub error_code: LoreErrorCode,
}

/// Terminal per-call event for `metadata_set`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoreRevisionTreeMetadataSetCompleteEventData {
    /// Correlation id of the originating call.
    pub id: u64,
    /// The outcome of the call.
    pub error_code: LoreErrorCode,
}

/// Per-call event carrying a metadata value from `metadata_get`. The
/// missing-key case emits no value event and lets the trailing `Complete`
/// fire on its own.
///
/// No `Debug` derive: the embedded `LoreMetadata` enum does not implement
/// `Debug`. Use `serde_json::to_string` to render this for diagnostics.
#[repr(C)]
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoreRevisionTreeMetadataGetCompleteEventData {
    /// Correlation id of the originating call.
    pub id: u64,
    /// The metadata key.
    pub key: LoreString,
    /// The metadata value.
    pub value: LoreMetadata,
    /// The outcome of the call.
    pub error_code: LoreErrorCode,
}

/// Terminal per-call event for `commit`. On success `revision_hash` is the
/// newly-committed revision and `new_tip_hash` is `Hash::default()`. When
/// `error_code` reports `BranchAdvanced`, `new_tip_hash` carries the
/// observed branch tip so the caller can reload without an extra
/// `branch::load_latest` round-trip.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoreRevisionTreeCommitCompleteEventData {
    /// Correlation id of the originating call.
    pub id: u64,
    /// The newly-committed revision.
    pub revision_hash: Hash,
    /// The observed branch tip when the branch had advanced.
    pub new_tip_hash: Hash,
    /// The outcome of the call.
    pub error_code: LoreErrorCode,
}

/// Terminal per-call event for `close`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoreRevisionTreeCloseCompleteEventData {
    /// Correlation id of the originating call.
    pub id: u64,
    /// The outcome of the call.
    pub error_code: LoreErrorCode,
}
