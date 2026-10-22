//! Platform skin and cape chooser, called only by the skin worker.

use launcher_host::desktop::picker::{self, Filter};
use std::path::PathBuf;

const SKINS: Filter = Filter {
    title: "Import skin PNG, geometry JSON, or skin pack",
    name: "Skins",
    extensions: &["png", "json", "mcpack"],
    unavailable_hint: "",
};

const CAPES: Filter = Filter {
    title: "Import cape PNG",
    name: "PNG images",
    extensions: &["png"],
    unavailable_hint: "",
};

/// Returns one chosen skin or cape file, `None` on cancellation, or a visible picker error.
pub(super) fn pick(cape: bool) -> Result<Option<PathBuf>, String> {
    picker::pick(if cape { &CAPES } else { &SKINS })
}
