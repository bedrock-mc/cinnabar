use super::super::{Descriptor, PackSources, RegistryRecord};
use super::context::RuleInputs;
use assets::{
    BLOCK_VISUAL_VARIANT_COVERED_GRASS, BLOCK_VISUAL_VARIANT_SNOW_COVER, BlockVisual,
    SNOWED_GRASS_SIDE_TEXTURE, VisualKind,
};

// Native grass_side's final variant (installed native vanilla base pack), selected
// when snow sits above.
// The samples' flattened grass_side retains only its newer overlay entry, but
// its mycelium_side array still exposes the same literal snowy sprite.
const GRASS_SIDE_KEY: &str = "grass_side";

pub(in crate::compiler) fn material_descriptor(
    pack: &PackSources,
) -> Option<(Descriptor, Box<str>)> {
    let native_side = pack
        .terrain
        .get_clamped_carried(GRASS_SIDE_KEY, usize::MAX)
        .filter(|(path, overlay)| *path == SNOWED_GRASS_SIDE_TEXTURE && overlay.is_none())
        .map(|(path, _)| path);
    let path = native_side.or_else(|| {
        pack.terrain
            .get_exact_pair_no_tint("mycelium_side")
            .map(|paths| paths[1])
            .filter(|path| *path == SNOWED_GRASS_SIDE_TEXTURE)
    })?;
    Some((
        Descriptor {
            path: path.into(),
            texture_key: GRASS_SIDE_KEY.into(),
            // Snow is precolored; never apply the grass alpha-overlay tint.
            flags: 0,
            state_variant: 0,
        },
        GRASS_SIDE_KEY.into(),
    ))
}

#[cfg(test)]
#[path = "snowy_grass/tests.rs"]
mod tests;

pub(in crate::compiler) fn apply(
    record: &RegistryRecord,
    visual: &mut BlockVisual,
    inputs: &RuleInputs<'_>,
) {
    if visual.kind == VisualKind::Diagnostic {
        return;
    }
    match record.name.as_ref() {
        "minecraft:grass_block" if visual.kind == VisualKind::Cube => {
            if let Some(material) = material_descriptor(inputs.pack)
                .and_then(|(descriptor, _)| inputs.material_by_descriptor.get(&descriptor))
            {
                visual.variant = BLOCK_VISUAL_VARIANT_COVERED_GRASS | *material;
            }
        }
        "minecraft:snow" | "minecraft:powder_snow" => {
            visual.variant |= BLOCK_VISUAL_VARIANT_SNOW_COVER;
        }
        _ => {}
    }
}
