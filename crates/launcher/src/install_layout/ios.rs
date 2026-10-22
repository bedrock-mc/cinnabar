//! iOS apps have a read-only bundle and one private container; the app host supplies both.

use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

use super::{InstallLayout, LayoutError};

static CONFIGURED: OnceLock<InstallLayout> = OnceLock::new();

impl InstallLayout {
    /// `resource_root` is the bundle directory that mirrors a checkout's `.local/`, carriers
    /// included: iOS cannot run the first-run compiler, so whoever builds the app adds the
    /// carriers they compiled, and the layout is not an installed one. Settings and data live in
    /// the container's `Documents`, which the Files app exposes, and per-run endpoints in its
    /// `tmp`. The core is linked into `executable` and runs in-process.
    #[must_use]
    pub fn ios(resource_root: PathBuf, container: &Path, executable: PathBuf) -> Self {
        let documents = container.join("Documents");
        let runtime_root = container.join("tmp/run");
        Self {
            compiled_assets: resource_root.join("assets/compiled"),
            physics_registry: resource_root
                .join("assets")
                .join(assets::carriers::physics_registry_basename()),
            resource_root,
            core_executable: executable,
            user_config_root: documents.join("config"),
            user_data_root: documents.join("data"),
            transient_runtime_root: runtime_root.clone(),
            runtime_root,
        }
    }
}

/// Records the iOS host's layout for [`InstallLayout::discover`]; it cannot change within a
/// process.
pub fn configure_ios(layout: InstallLayout) -> Result<(), LayoutError> {
    if *CONFIGURED.get_or_init(|| layout.clone()) == layout {
        Ok(())
    } else {
        Err(LayoutError::IosReconfigured)
    }
}

pub(super) fn discover() -> Result<InstallLayout, LayoutError> {
    CONFIGURED
        .get()
        .cloned()
        .ok_or(LayoutError::IosUnconfigured)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bundled carriers load in place without first-run preparation, while everything the app
    /// writes stays in its container.
    #[test]
    fn ios_reads_bundled_carriers_and_writes_only_to_its_container() {
        let bundle = Path::new("/private/var/containers/Bundle/Application/X/Cinnabar.app");
        let resources = bundle.join("local");
        let container = Path::new("/private/var/mobile/Containers/Data/Application/Y");
        let executable = bundle.join("Cinnabar");
        let layout = InstallLayout::ios(resources.clone(), container, executable.clone())
            .with_prepared_assets();
        assert!(!layout.is_installed());
        assert_eq!(layout.compiled_assets, resources.join("assets/compiled"));
        assert_eq!(
            layout.vanilla_pack_dir(),
            resources.join(super::super::vanilla_pack_relative())
        );
        assert!(layout.physics_registry.starts_with(&resources));
        assert_eq!(layout.core_executable, executable);
        for written in [
            layout.user_config_root.clone(),
            layout.user_data_root.clone(),
            layout.log_dir(),
            layout.auth_cache(),
            layout.transient_runtime_root().to_path_buf(),
        ] {
            assert!(written.starts_with(container), "{}", written.display());
        }
    }
}
