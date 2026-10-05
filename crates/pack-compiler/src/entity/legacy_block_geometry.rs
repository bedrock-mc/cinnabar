use super::*;

/// Retains the block model still supplied in the legacy catalog without admitting obsolete rigs.
pub(super) fn parse(
    relative_path: &str,
    path: &Path,
    value: &Value,
    symbols: &mut BTreeMap<(EntityAssetKind, Box<str>, Box<str>), PendingSymbol>,
    geometries: &mut BTreeMap<(Box<str>, Box<str>), PendingGeometry>,
) -> Result<(), AssetError> {
    let Some(bed) = value.get(assets::BED_GEOMETRY_IDENTIFIER) else {
        return Ok(());
    };
    let mut selected = serde_json::Map::new();
    if let Some(version) = value.get("format_version") {
        selected.insert("format_version".into(), version.clone());
    }
    selected.insert(assets::BED_GEOMETRY_IDENTIFIER.into(), bed.clone());
    parse_geometry(
        relative_path,
        path,
        &Value::Object(selected),
        symbols,
        geometries,
    )
}
