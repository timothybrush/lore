// SPDX-FileCopyrightText: 2026 Epic Games, Inc.
// SPDX-License-Identifier: MIT
use serde::Deserialize;
use serde::Serialize;

use crate::event::LoreEvent;
use crate::interface::LoreString;

/// Event data naming a path that was ignored or could not be resolved.
#[repr(C)]
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LorePathIgnoreEventData {
    /// The ignored path
    pub path: LoreString,
}

pub async fn emit_path_ignore(path: &str) {
    LoreEvent::PathIgnore(LorePathIgnoreEventData { path: path.into() }).send();
}
