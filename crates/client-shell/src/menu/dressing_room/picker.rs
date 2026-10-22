//! Platform skin and cape chooser, called only by the skin worker.

use crate::player_skin::catalog::MAX_PNG_BYTES;
use launcher_host::desktop::picker::{self, Filter};
use std::path::PathBuf;

/// The largest file a skin import reads: a skin pack, a PNG or a geometry JSON.
const MAX_SKIN_BYTES: u64 = larger(
    larger(resource_pack::MAX_ARCHIVE_BYTES as u64, MAX_PNG_BYTES),
    render_api::MAX_SKIN_GEOMETRY_SOURCE_BYTES as u64,
);

const SKINS: Filter = Filter {
    title: "Import skin PNG, geometry JSON, or skin pack",
    name: "Skins",
    extensions: &["png", "json", "mcpack"],
    unavailable_hint: "",
    max_bytes: Some(MAX_SKIN_BYTES),
};

const CAPES: Filter = Filter {
    title: "Import cape PNG",
    name: "PNG images",
    extensions: &["png"],
    unavailable_hint: "",
    max_bytes: Some(MAX_PNG_BYTES),
};

/// The larger of two limits, usable in constants.
const fn larger(a: u64, b: u64) -> u64 {
    if a > b { a } else { b }
}

/// Returns one chosen skin or cape file, `None` on cancellation, or a visible picker error.
pub(super) fn pick(cape: bool) -> Result<Option<PathBuf>, String> {
    picker::pick(if cape { &CAPES } else { &SKINS })
}
