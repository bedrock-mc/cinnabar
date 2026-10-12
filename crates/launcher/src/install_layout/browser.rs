//! The browser's install layout: a fixed virtual tree, since a page has no executable, user
//! directories or process id.

use std::path::PathBuf;

use super::InstallLayout;
#[cfg(target_arch = "wasm32")]
use super::LayoutError;

/// Where the browser's virtual layout is rooted.
pub const BROWSER_ROOT: &str = "/cinnabar";

/// This process's id, which names its runtime files. A browser page has no process id; its one
/// client is process 0.
#[must_use]
pub fn process_id() -> u32 {
    #[cfg(not(target_arch = "wasm32"))]
    return std::process::id();
    #[cfg(target_arch = "wasm32")]
    0
}

impl InstallLayout {
    /// The browser's layout. The page preloads the carriers under `compiled_assets` and the
    /// physics registry before startup.
    #[must_use]
    pub fn browser() -> Self {
        let root = PathBuf::from(BROWSER_ROOT);
        Self {
            compiled_assets: root.join("assets/compiled"),
            physics_registry: root
                .join("assets")
                .join(assets::carriers::physics_registry_basename()),
            core_executable: root.join("bedrock-core"),
            user_config_root: root.join("config"),
            user_data_root: root.join("data"),
            runtime_root: root.join("run"),
            transient_runtime_root: root.join("run"),
            resource_root: root,
        }
    }

    /// The layout this process runs from; a browser's is [`Self::browser`].
    #[cfg(target_arch = "wasm32")]
    pub fn discover() -> Result<Self, LayoutError> {
        Ok(Self::browser())
    }
}
