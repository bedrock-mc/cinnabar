#![project(id = "cosmic", permissions(messaging, ui))]

use experience_sdk::{id, message::{BlockId, ItemId}};

#[block(name = "Moon rock", texture = assets::ASSET_MOON_PNG, hardness = 2.0)]
pub const MOON_ROCK: BlockId = id!("cosmic:moon_rock");

#[item(name = "Moon key", icon = assets::ASSET_MOON_PNG, max_stack = 1)]
pub const MOON_KEY: ItemId = id!("cosmic:moon_key");

#[message(id = "cosmic.ping", schema = 1, to_server)]
pub struct Ping {
    pub request: u32,
}

#[message(id = "cosmic.pong", schema = 1, to_client)]
pub struct Pong {
    pub request: u32,
}
