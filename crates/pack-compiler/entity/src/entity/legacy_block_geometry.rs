use super::*;

/// Retains models still supplied only in the legacy catalog without admitting obsolete actor rigs.
pub(super) fn parse(
    relative_path: &str,
    path: &Path,
    value: &Value,
    symbols: &mut BTreeMap<(EntityAssetKind, Box<str>, Box<str>), PendingSymbol>,
    geometries: &mut BTreeMap<(Box<str>, Box<str>), PendingGeometry>,
) -> Result<(), AssetError> {
    let selected = select(value);
    if !selected
        .as_object()
        .is_some_and(|map| map.keys().any(|key| key.starts_with("geometry.")))
    {
        return Ok(());
    }
    parse_geometry(relative_path, path, &selected, symbols, geometries)
}

/// Keeps catalog-only block and equipment models in both compiled and reference payloads.
pub(super) fn select(value: &Value) -> Value {
    let mut selected = serde_json::Map::new();
    if let Some(version) = value.get("format_version") {
        selected.insert("format_version".into(), version.clone());
    }
    for identifier in [
        assets::BED_GEOMETRY_IDENTIFIER,
        assets::CAPE_GEOMETRY_IDENTIFIER,
        assets::ELYTRA_GEOMETRY_IDENTIFIER,
    ] {
        if let Some(geometry) = value.get(identifier) {
            selected.insert(identifier.into(), geometry.clone());
        }
    }
    Value::Object(selected)
}
