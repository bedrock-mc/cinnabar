//! Vanilla screen families drawn through the JSON-UI engine: the container screens with their
//! recipe and furnace books, and the gameplay HUD with its native renderers. Screens read typed,
//! borrowed inputs the client's adapters project from its runtime and return what they drew.

pub mod book_screen;
pub mod container_actions;
pub mod container_data;
pub mod container_kinds;
pub mod container_source;
pub mod containers;
pub mod furnace_book;
pub mod hud;
pub mod hud_renderers;
pub mod recipe_book;
