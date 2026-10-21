use std::{collections::BTreeMap, path::Path};

use super::safe_relative;

/// A terrain catalog image and its literal atlas-space RGB multiplier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::pbr) struct TerrainSource {
    pub path: String,
    pub tint: Option<[u8; 3]>,
}

/// Reads supported authored atlas sources through the compiler's shared terrain resolver.
pub(in crate::pbr) fn read(root: &Path) -> Result<BTreeMap<String, TerrainSource>, String> {
    let path = root.join("textures/terrain_texture.json");
    if !path.is_file() {
        return Ok(BTreeMap::new());
    }
    let terrain =
        pack_compile_input::pack::read_terrain(&path).map_err(|error| error.to_string())?;
    let mut result = BTreeMap::new();
    for (key, value, tint) in terrain.fixed_tint_sources() {
        let relative = safe_relative(value)
            .ok_or_else(|| format!("terrain texture {key} escapes its defining pack"))?;
        result.insert(
            key.to_owned(),
            TerrainSource {
                path: relative.to_string_lossy().replace('\\', "/"),
                tint,
            },
        );
    }
    Ok(result)
}
