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

/// The community Discord invite the client links to for help.
pub const DISCORD_INVITE_URL: &str = "https://discord.gg/MeEz7BEHcM";

/// The packaged client version from the workspace manifest.
pub const PRODUCT_VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod accounts;
pub mod branding;
pub mod download;
pub mod dressing_room;
pub mod global_resources;
pub mod graphics_backend;
pub mod install_layout;
pub mod local_worlds;
pub mod marketplace;
pub mod menu;
pub mod minecraft_import;
pub mod mods;
pub mod shader_options;
pub mod skin_import;
pub mod startup;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

/// Embedders may supply a title without changing the client's installation identity.
pub fn window_title(override_title: Option<&str>) -> String {
    override_title
        .filter(|title| !title.trim().is_empty())
        .unwrap_or(PRODUCT_NAME)
        .to_owned()
}

#[cfg(test)]
mod embedding_tests {
    #[test]
    fn window_title_accepts_nonempty_override_and_keeps_default() {
        for value in [None, Some(""), Some("  ")] {
            assert_eq!(super::window_title(value), super::PRODUCT_NAME);
        }
        assert_eq!(super::window_title(Some("Zeno Client")), "Zeno Client");
    }
}
