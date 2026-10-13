//! Per-user paths for optional client services.

use super::{MenuRuntime, PathBuf};

impl MenuRuntime {
    /// Server trust lives beside the other per-user launcher settings.
    pub fn experience_settings_path(&self) -> PathBuf {
        self.config_path
            .with_file_name(server_experience::trust::SETTINGS_FILE)
    }

    /// The immutable bundle cache follows the installed per-user data layout.
    pub fn experience_cache_dir(&self) -> PathBuf {
        server_experience::cache::objects_dir(&self.layout.user_data_root)
    }
}
