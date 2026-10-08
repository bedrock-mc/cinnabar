//! Bindings for native held models whose samples supply art and clips without an attachable.

use super::*;

const SPYGLASS: &str = "minecraft:spyglass";
const SOURCE: &str = "attachables/_cinnabar/spyglass.json";

/// Completes only the startup carrier; authored attachables and server packs keep their binding.
pub(super) fn complete_sample_bindings(
    root: &Path,
    sources: &mut Vec<EntityAssetSource>,
    payloads: &mut SourcePayloads,
    symbols: &mut BTreeMap<(EntityAssetKind, Box<str>, Box<str>), PendingSymbol>,
    geometries: &mut BTreeMap<(Box<str>, Box<str>), PendingGeometry>,
) -> Result<usize, AssetError> {
    if symbols.values().any(|symbol| {
        symbol.kind == EntityAssetKind::Attachable && symbol.identifier.as_ref() == SPYGLASS
    }) || ![
        "models/entity/spyglass.geo.json",
        "animations/spyglass.animation.json",
        "textures/entity/spyglass.png",
    ]
    .iter()
    .all(|path| payloads.contains_key(*path))
    {
        return Ok(0);
    }
    let bytes = serde_json::to_vec(&spyglass_binding())
        .map_err(|_| invalid("native held-item binding could not be encoded"))?;
    parse_source(SOURCE, &root.join(SOURCE), &bytes, symbols, geometries)?;
    sources.push(EntityAssetSource {
        path: SOURCE.into(),
        source_bytes: bytes.len() as u32,
        source_sha256: Sha256::digest(&bytes).into(),
    });
    let size = bytes.len();
    payloads.insert(SOURCE.into(), bytes.into());
    Ok(size)
}

/// Connects the sample's authored geometry, texture and both poses without reproducing their art.
fn spyglass_binding() -> Value {
    serde_json::json!({
        "format_version": "1.10.0",
        "minecraft:attachable": {"description": {
            "identifier": SPYGLASS,
            "materials": {
                "default": "entity_alphatest", "enchanted": "entity_alphatest_glint"
            },
            "textures": {
                "default": "textures/entity/spyglass",
                "enchanted": "textures/misc/enchanted_item_glint"
            },
            "geometry": {"default": "geometry.spyglass"},
            "animations": {
                "holding": "animation.spyglass.holding",
                "scoping": "animation.spyglass.scoping"
            },
            "scripts": {"animate": [
                {"holding": "query.main_hand_item_use_duration <= 0.0"},
                {"scoping": "query.main_hand_item_use_duration > 0.0 && !context.is_first_person"}
            ]},
            "render_controllers": ["controller.render.item_default"]
        }}
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Loads installed runtime-only references; an absent pack is a named fixture skip.
    fn sample_pack() -> Option<PathBuf> {
        let Some(root) = std::env::var_os("CINNABAR_VANILLA_RESOURCE_PACK") else {
            eprintln!(
                "skipping spyglass fixture: CINNABAR_VANILLA_RESOURCE_PACK is not configured"
            );
            return None;
        };
        Some(root.into())
    }

    #[test]
    fn sample_spyglass_binds_its_model_texture_and_holding_and_scoping_clips() {
        let Some(root) = sample_pack() else { return };
        let compiled =
            compile_entity_assets_with_report(&root, assets::VANILLA_SOURCE_MANIFEST.as_bytes())
                .unwrap();
        let binding = compiled
            .equipment_bindings
            .iter()
            .find(|binding| binding.identifier.as_ref() == SPYGLASS)
            .expect("the sample spyglass must be drawable as an attachable");
        assert_eq!(binding.geometry.identifier.as_ref(), "geometry.spyglass");
        assert_eq!(
            binding.texture.identifier.as_ref(),
            "textures/entity/spyglass"
        );
        let runtime = assets::RuntimeEntityAssets::from_compiled(compiled.assets).unwrap();
        let symbol = runtime
            .symbols()
            .iter()
            .position(|symbol| {
                symbol.kind == EntityAssetKind::Attachable && symbol.identifier.as_ref() == SPYGLASS
            })
            .unwrap();
        let rig = runtime
            .rig_bindings()
            .iter()
            .find(|rig| rig.entity_symbol as usize == symbol)
            .expect("spyglass needs an animation rig, not its inventory sprite");
        let geometry = &runtime.rig_geometries()[rig.first_geometry as usize];
        let animations = &runtime.rig_animations()[geometry.first_animation as usize
            ..(geometry.first_animation + u32::from(geometry.animation_count)) as usize];
        let names = animations
            .iter()
            .map(|animation| {
                runtime.symbols()
                    [runtime.animation_clips()[animation.clip as usize].symbol as usize]
                    .identifier
                    .as_ref()
            })
            .collect::<Vec<_>>();
        assert!(names.contains(&"animation.spyglass.holding"));
        assert!(names.contains(&"animation.spyglass.scoping"));
    }
}
