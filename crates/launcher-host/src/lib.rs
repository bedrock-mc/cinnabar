//! Launcher process, platform, persistence and background services without an engine dependency.

pub mod account_control;
pub mod accounts;
pub mod auth;
pub mod catalog;
pub mod catalog_art;
#[cfg_attr(target_arch = "wasm32", path = "client_random_id_web.rs")]
pub mod client_random_id;
pub mod core_process;
pub mod desktop;
pub mod launcher_account;
pub mod launcher_core;
pub mod lifecycle;
pub mod local_worlds;
pub mod native_dialog;
#[cfg(target_arch = "wasm32")]
pub mod relay_account;
pub mod remote_images;
pub mod server_trust;
pub mod servers;
pub mod session_cleanup;
pub mod settings_storage;
pub mod sign_in_browser;
pub mod video_settings;

pub mod memory;
