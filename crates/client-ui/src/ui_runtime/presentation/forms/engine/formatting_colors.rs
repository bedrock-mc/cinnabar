//! The shared text palette follows the merged UI catalog, including server pack globals.

use json_ui::Catalog;
use ui::FormattingPalette;

/// Resolves vanilla's first three array elements as RGB, ignoring an optional alpha.
pub(super) fn from_catalog(catalog: &Catalog) -> FormattingPalette {
    FormattingPalette::from_globals(|name| {
        let values = catalog.global(name.strip_prefix('$')?)?.as_array()?;
        if values.len() < 3 {
            return None;
        }
        Some(std::array::from_fn(|index| {
            values[index].as_f64().unwrap_or(0.0) as f32
        }))
    })
}
