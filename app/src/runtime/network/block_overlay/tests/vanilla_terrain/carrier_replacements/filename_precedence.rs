use super::super::super::super::{Builder, MIN_TILE, TextureRoute, textures::TextureCatalog};
use super::*;

const PNG_COLOR: [u8; 4] = [230, 40, 20, 255];
const TGA_COLOR: [u8; 4] = [20, 40, 230, 255];

/// Supplies distinct PNG and TGA donors and independent filler routes for capacity checks.
fn donor_keys() -> MaterialKeys {
    MaterialKeys::from_entries([(1, "png"), (2, "tga"), (3, "fill_a"), (4, "fill_b")])
        .with_aliases([
            ("bare", "textures/example"),
            ("png", "textures/example.png"),
            ("tga", "textures/example.tga"),
            ("fill_a", "textures/fill_a"),
            ("fill_b", "textures/fill_b"),
        ])
        .with_sources(
            [],
            [
                (1, "png", "textures/example.png"),
                (2, "tga", "textures/example.tga"),
                (3, "fill_a", "textures/fill_a.png"),
                (4, "fill_b", "textures/fill_b.png"),
            ]
            .map(|(id, key, raster)| {
                (
                    id,
                    MaterialTextureSource {
                        key: key.into(),
                        variant: 0,
                        path: raster.into(),
                        raster: raster.into(),
                        baked_tint: None,
                    },
                )
            }),
            [],
        )
}

/// Starts carrier-enabled admission with only the diagnostic layer retained.
fn builder<'a>(
    view: &'a LayeredPackView,
    base: &'a RuntimeAssets,
    keys: &MaterialKeys,
    layers: usize,
) -> Builder<'a> {
    let policy = assets::texture::policy::device_policy();
    Builder {
        fallback: None,
        base_keys: None,
        catalog: TextureCatalog::new(view, Some(keys)),
        base_assets: Some(base),
        vanilla_carried: Vec::new(),
        terrain_materials: Default::default(),
        overlay: Default::default(),
        sources: vec![(super::super::super::super::Source::Diagnostic, 0)],
        source_bytes: 0,
        tiles: Vec::new(),
        upload_bytes: policy.terrain_bytes(MIN_TILE, 1).unwrap(),
        texture_policy: policy,
        max_layers: layers,
        slots: Default::default(),
        materials: Default::default(),
        visuals: Default::default(),
        gaps: Default::default(),
    }
}

/// Requests a carrier donor through the production route and frame caches.
fn route(key: &str) -> TextureRoute {
    TextureRoute::Catalog {
        key: key.to_owned(),
        variant: 0,
        choice: 0,
    }
}

/// Reads a retained shard's first texel for the admitted route's reference.
fn pixel(overlay: &assets::BlockOverlay, reference: assets::TextureRef) -> [u8; 4] {
    let mip = &overlay.textures[reference.page() as usize - 1].mips[0];
    let start = reference.layer() as usize * (mip.size * mip.size * 4) as usize;
    mip.rgba8[start..start + 4].try_into().unwrap()
}

#[test]
fn extensionless_carrier_routes_choose_tga_independently_of_cached_png_order() {
    let base = carrier(&[PNG_COLOR, TGA_COLOR], [1; 6], &[0; 4]);
    let keys = donor_keys();
    let view = stack(&[(json!({}), json!({}), vec![])]);
    for order in [["png", "bare"], ["bare", "png"]] {
        let mut builder = builder(&view, &base, &keys, 2);
        let mut references = Vec::new();
        for key in order {
            let slot = builder.carrier_slot(route(key), &keys).unwrap();
            references.push((key, assets::TextureRef::new(slot.page, slot.layer).unwrap()));
        }
        let compiled = builder.finish().unwrap();
        for (key, reference) in references {
            assert_eq!(
                pixel(&compiled.overlay, reference),
                if key == "bare" { TGA_COLOR } else { PNG_COLOR }
            );
        }
    }
}

#[test]
fn full_arrays_do_not_substitute_a_cached_lower_priority_carrier_filename() {
    let base = carrier(&[PNG_COLOR, TGA_COLOR], [1; 6], &[0; 4]);
    let keys = donor_keys();
    let view = stack(&[(json!({}), json!({}), vec![])]);
    let mut builder = builder(&view, &base, &keys, 1);
    for key in ["png", "fill_a", "fill_b"] {
        builder.carrier_slot(route(key), &keys).unwrap();
    }
    let attempts = builder.catalog.decode_attempts.get();
    assert!(builder.carrier_slot(route("bare"), &keys).is_none());
    assert_eq!(builder.gaps.saturated_textures, 1);
    assert_eq!(builder.catalog.decode_attempts.get(), attempts);
    assert!(builder.carrier_slot(route("png"), &keys).is_some());
}

#[test]
fn full_arrays_reuse_the_cached_winning_carrier_filename() {
    let base = carrier(&[PNG_COLOR, TGA_COLOR], [1; 6], &[0; 4]);
    let keys = donor_keys();
    let view = stack(&[(json!({}), json!({}), vec![])]);
    let mut builder = builder(&view, &base, &keys, 1);
    let first = builder.carrier_slot(route("bare"), &keys).unwrap();
    for key in ["png", "fill_a"] {
        builder.carrier_slot(route(key), &keys).unwrap();
    }
    let attempts = builder.catalog.decode_attempts.get();
    let reused = builder.carrier_slot(route("tga"), &keys).unwrap();
    assert_eq!((reused.page, reused.layer), (first.page, first.layer));
    assert_eq!(builder.catalog.decode_attempts.get(), attempts);
    assert_eq!(builder.gaps.saturated_textures, 0);
}
