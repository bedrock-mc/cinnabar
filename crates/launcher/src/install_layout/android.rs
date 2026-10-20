//! Android installs have no executable-relative layout; the activity supplies its private roots.

use std::{path::Path, sync::OnceLock};

use super::{InstallLayout, LayoutError, Platform, local_server_filename};

static CONFIGURED: OnceLock<InstallLayout> = OnceLock::new();

impl InstallLayout {
    /// Private app files hold settings, data and the unpacked prep kit; helpers stay in the
    /// APK's native-library directory, the only place Android lets an app execute files.
    #[must_use]
    pub fn android(files_dir: &Path, native_library_dir: &Path, core_library: &str) -> Self {
        let resource_root = files_dir.join("resources");
        let runtime_root = files_dir.join("run");
        Self {
            compiled_assets: resource_root.join("assets"),
            physics_registry: resource_root
                .join("assets")
                .join(assets::carriers::physics_registry_basename()),
            resource_root,
            core_executable: native_library_dir.join(core_library),
            local_server_executable: native_library_dir
                .join(local_server_filename(Platform::Linux)),
            user_config_root: files_dir.join("config"),
            user_data_root: files_dir.join("data"),
            transient_runtime_root: runtime_root.clone(),
            runtime_root,
        }
    }
}

/// Records the Android activity's layout for [`InstallLayout::discover`]; it cannot change
/// within a process.
pub fn configure_android(layout: InstallLayout) -> Result<(), LayoutError> {
    if *CONFIGURED.get_or_init(|| layout.clone()) == layout {
        Ok(())
    } else {
        Err(LayoutError::AndroidReconfigured)
    }
}

pub(super) fn discover() -> Result<InstallLayout, LayoutError> {
    CONFIGURED
        .get()
        .cloned()
        .map(InstallLayout::with_prepared_assets)
        .ok_or(LayoutError::AndroidUnconfigured)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Carriers are prepared into private data, while helpers run from the native directory.
    #[test]
    fn android_separates_private_data_from_executable_helpers() {
        let files = Path::new("/data/user/0/app/files");
        let core = Path::new("/data/app/lib/arm64/libbedrock_core.so");
        let native_library_dir = core.parent().unwrap();
        let layout = InstallLayout::android(
            files,
            native_library_dir,
            core.file_name().unwrap().to_str().unwrap(),
        )
        .with_prepared_assets();
        assert!(layout.is_installed());
        assert_eq!(layout.core_executable, core);
        assert_eq!(
            layout.local_server_executable,
            native_library_dir.join("bedrock-local-server")
        );
        assert_eq!(layout.prep_kit(), files.join("resources/prep-kit"));
        assert_eq!(layout.compiled_assets, files.join("data/assets/compiled"));
        assert_eq!(layout.user_config_root, files.join("config"));
        assert!(layout.transient_runtime_root().starts_with(files));
    }
}
