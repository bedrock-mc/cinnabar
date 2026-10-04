//! Engine-independent launcher models shared by app services and client presentation.

/// The product name as a literal, for `concat!` in compile-time text.
#[macro_export]
macro_rules! product_name {
    () => {
        "Cinnabar"
    };
}

/// The product name shown in window titles, defaults and the install directory.
pub const PRODUCT_NAME: &str = product_name!();

pub mod global_resources;
pub mod install_layout;
pub mod local_worlds;
pub mod menu;
pub mod store;
