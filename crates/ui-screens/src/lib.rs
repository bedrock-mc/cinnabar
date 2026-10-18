//! Vanilla screen families drawn through the JSON-UI engine: the container screens with their
//! recipe and furnace books, the HUD with its native renderers, chat, signs, NPC dialogue, the
//! credits, loading, toasts, the roster, the emote wheel and the menu shell's screen bindings.
//! Screens read typed, borrowed inputs the client's adapters project from its runtime and
//! return what they drew.

pub mod always_sprint_setting;
pub mod book_screen;
pub mod chat_coordinates;
pub mod chat_links;
pub mod chat_position;
pub mod chat_screen;
pub mod container_actions;
pub mod container_data;
pub mod container_kinds;
pub mod container_source;
pub mod containers;
pub mod credits_content;
mod credits_renderer;
pub mod credits_screen;
pub mod crosshair_settings;
pub mod death_screen;
pub mod disconnect_retry;
pub mod discord_presence_setting;
pub mod emote_screen;
pub mod enhanced_setting;
pub mod furnace_book;
pub mod global_resources;
pub mod graphics_expander;
pub mod hud;
pub mod hud_renderers;
pub mod inventory_capture;
pub mod invite_screen;
pub mod java_animations_setting;
pub mod java_world_visuals_setting;
pub mod join_progress;
pub mod loading_screen;
pub mod menu_caret;
pub mod menu_policy;
pub mod menu_renderers;
pub mod menu_screens;
pub mod motion_blur_setting;
pub mod npc;
pub mod play_screen;
pub mod player_list;
pub mod recipe_book;
pub mod regions;
pub mod settings_account;
pub mod settings_chat;
pub mod settings_controls;
pub mod settings_defaults;
pub mod settings_keys;
pub mod settings_language;
pub mod settings_reset;
pub mod settings_resources;
pub mod settings_storage;
pub mod settings_support;
pub mod sign_editor;
pub mod skybox_setting;
pub mod smaa_setting;
pub mod start_feed;
pub mod text;
pub mod toast_screen;
pub mod vrr_setting;
pub mod vsync_setting;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
