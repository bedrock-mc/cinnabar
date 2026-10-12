//! Bounded local panel catalogs prepared independently of component callbacks.

use std::{fs::File, io::Read, path::Path};

/// Optional local catalog selected for a directly loaded personal component.
pub const UI_ENV: &str = "CINNABAR_MOD_UI";

/// Reads a bounded catalog source; callers validate it on their loading worker.
pub fn load(path: &Path) -> Result<String, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| {
            file.take(ui::mod_panel::MAX_PANEL_BUNDLE_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
        })
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if bytes.len() > ui::mod_panel::MAX_PANEL_BUNDLE_BYTES {
        return Err("panel bundle exceeds byte limit".into());
    }
    String::from_utf8(bytes).map_err(|error| error.to_string())
}
