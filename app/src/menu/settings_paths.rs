//! Per-user paths for optional client services.

use super::{MenuRuntime, PathBuf};

impl MenuRuntime {
    /// Where the Marketplace settings file lives.
    pub(crate) fn store_settings_path(&self) -> PathBuf {
        self.config_path
            .with_file_name(launcher::store::settings::SETTINGS_FILE)
    }

    /// Where downloaded Marketplace art is cached.
    pub(crate) fn store_images_dir(&self) -> PathBuf {
        self.layout.store_images_dir()
    }

    /// Server trust lives beside the other per-user launcher settings.
    pub(crate) fn experience_settings_path(&self) -> PathBuf {
        self.config_path
            .with_file_name(server_experience::trust::SETTINGS_FILE)
    }

    /// The immutable bundle cache follows the installed per-user data layout.
    pub(crate) fn experience_cache_dir(&self) -> PathBuf {
        self.layout.experience_cache_dir()
    }
}
