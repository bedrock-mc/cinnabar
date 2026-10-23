#![project(id = "terminal", permissions(messaging, ui))]
use experience_sdk::{id, message::{BlockId, ItemId, EntityId, WorldId}};

/// A full cube backed by a generated texture constant.
#[block(name = "Terminal", texture = assets::ASSET_TERMINAL_PNG, hardness = 1.0)]
pub const TERMINAL: BlockId = id!("terminal:terminal");

/// A portable item backed by the same original icon.
#[item(name = "Terminal key", icon = assets::ASSET_TERMINAL_PNG, max_stack = 1)]
pub const KEY: ItemId = id!("terminal:key");

/// Reserved definition kinds for future entity and world registration.
pub const DISPLAY: EntityId = id!("terminal:display");
/// A definition key, distinct from a live world instance or network dimension ID.
pub const HOME: WorldId = id!("terminal:home");
