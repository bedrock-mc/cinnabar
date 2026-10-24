//! Launcher process, platform, persistence and background services without an engine dependency.

pub mod account_control;
pub mod accounts;
pub mod auth;
pub mod catalog_art;
pub mod client_random_id;
pub mod desktop;
pub mod destination;
#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
pub mod launcher_account;
pub mod lifecycle;
pub mod local_worlds;
#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
pub mod native_account;
pub mod native_dialog;
#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
mod product_api;
pub mod remote_images;
pub mod renderer_startup;
pub mod server_trust;
pub mod servers;
pub mod session_cleanup;
pub mod settings_storage;
pub mod sign_in_browser;
pub mod video_settings;
#[cfg(any(test, all(feature = "native-networking", not(target_arch = "wasm32"))))]
mod worker;

pub mod memory;
pub mod minecraft_import;
pub mod mod_updates;

pub mod cosmetics;
