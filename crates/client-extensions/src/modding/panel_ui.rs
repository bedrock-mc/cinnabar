//! Bounded local panel catalogs prepared independently of component callbacks.

use std::path::Path;

/// Optional local catalog selected for a directly loaded personal component.
pub const UI_ENV: &str = "CINNABAR_MOD_UI";

/// Reads a bounded catalog source; callers validate it on their loading worker.
pub fn load(path: &Path) -> Result<String, String> {
    let bytes = mod_host::local_files::read(path, ui::mod_panel::MAX_PANEL_BUNDLE_BYTES)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    String::from_utf8(bytes).map_err(|error| error.to_string())
}
