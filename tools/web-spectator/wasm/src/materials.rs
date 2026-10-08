use assets::{
    BlockFlags, BlockOverlay, BlockVisual, ContributorRole, LightProperties, Material,
    NO_ANIMATION, NO_MODEL_TEMPLATE, RuntimeAssets, TextureArray, TextureMip, TextureRef,
    VisualKind, VisualSupport,
};

use crate::model::PaletteEntry;

pub(super) fn palette_assets(
    palette: &[PaletteEntry],
) -> Result<(RuntimeAssets, Vec<[f32; 3]>), String> {
    let colors = palette.iter().map(block_color).collect::<Vec<_>>();
    let mut overlay = BlockOverlay::default();
    for index in 1..palette.len() {
        let material = (index - 1) as u32;
        overlay.visuals.push(BlockVisual {
            faces: [material; 6],
            flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
            kind: VisualKind::Diagnostic,
            support: VisualSupport::Diagnostic,
            contributor_role: ContributorRole::Primary,
            model_template: NO_MODEL_TEMPLATE,
            animation: NO_ANIMATION,
            variant: 0,
        });
        overlay.light_properties.push(LightProperties::OPAQUE_DARK);
        overlay.materials.push(Material {
            texture: TextureRef::new(1, material).map_err(|error| error.to_string())?,
            flags: 0,
            animation: NO_ANIMATION,
            ..Material::unvaried()
        });
    }
    if palette.len() > 1 {
        overlay.texture = Some(TextureArray {
            layers: (palette.len() - 1) as u32,
            mips: vec![TextureMip {
                size: 1,
                rgba8: vec![255; (palette.len() - 1) * 4].into_boxed_slice(),
            }]
            .into_boxed_slice(),
        });
    }
    let assets = RuntimeAssets::diagnostic()
        .with_block_overlay(1, &overlay)
        .map_err(|error| format!("invalid arena material palette: {error}"))?;
    Ok((assets, colors))
}

// Deliberately diagnostic flat colors, not sampled Mojang textures. Named
// families keep stone/wood/foliage legible; states retain dyed material colors.
fn block_color(entry: &PaletteEntry) -> [f32; 3] {
    let name = entry.name.strip_prefix("minecraft:").unwrap_or(&entry.name);
    let dyed = ["wool", "concrete", "terracotta", "glass", "carpet"]
        .into_iter()
        .any(|family| name.contains(family));
    let rgb = if dyed {
        dye_color(entry, name).unwrap_or([177, 173, 165])
    } else if name.contains("grass") || name.contains("leaves") || name.contains("melon") {
        [94, 145, 69]
    } else if name.contains("sand") || name.contains("end_stone") {
        [216, 201, 147]
    } else if name.contains("snow") || name.contains("quartz") {
        [224, 228, 226]
    } else if name.contains("ice") || name.contains("water") {
        [101, 168, 204]
    } else if name.contains("lava") {
        [231, 101, 35]
    } else if name.contains("netherrack") || name.contains("nether_brick") {
        [104, 53, 57]
    } else if name.contains("obsidian") || name.contains("blackstone") {
        [49, 44, 61]
    } else if name.contains("dirt") || name.contains("mud") {
        [126, 95, 67]
    } else if name.contains("wood") || name.contains("planks") || name.contains("log") {
        [167, 129, 76]
    } else if name.contains("brick") {
        [166, 100, 78]
    } else if name.contains("gold") {
        [227, 189, 62]
    } else if name.contains("diamond") {
        [65, 195, 202]
    } else if name.contains("copper") {
        [179, 114, 80]
    } else {
        [140, 146, 151]
    };
    rgb.map(|channel| {
        let srgb = f32::from(channel) / 255.;
        if srgb <= 0.04045 {
            srgb / 12.92
        } else {
            ((srgb + 0.055) / 1.055).powf(2.4)
        }
    })
}

fn dye_color(entry: &PaletteEntry, name: &str) -> Option<[u8; 3]> {
    let state = entry
        .states
        .get("color")
        .and_then(serde_json::Value::as_str);
    const DYES: [(&str, [u8; 3]); 16] = [
        ("light_blue", [86, 169, 208]),
        ("light_gray", [160, 160, 153]),
        ("light_grey", [160, 160, 153]),
        ("orange", [225, 134, 47]),
        ("magenta", [179, 76, 179]),
        ("yellow", [233, 205, 68]),
        ("purple", [128, 62, 166]),
        ("green", [85, 110, 49]),
        ("brown", [116, 81, 52]),
        ("white", [228, 228, 222]),
        ("black", [40, 42, 45]),
        ("pink", [219, 141, 168]),
        ("blue", [65, 75, 151]),
        ("cyan", [58, 132, 143]),
        ("lime", [127, 176, 61]),
        ("red", [171, 59, 52]),
    ];
    DYES.into_iter()
        .find(|(dye, _)| state == Some(*dye) || name.starts_with(&format!("{dye}_")))
        .map(|(_, color)| color)
        .or_else(|| (state == Some("gray") || name.starts_with("gray_")).then_some([81, 85, 90]))
}
