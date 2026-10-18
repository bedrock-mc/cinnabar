//! World model sources keep their original resolution and directional UVs in actual slot setup.

use super::*;

const ITEM: &str = "fixture:world_cube";
const VISUAL: assets::BlockVisualId = assets::BlockVisualId(2);

/// Replaces the complete fixture's full-cube texture with original high-resolution art and rotated UVs.
fn world(entities: &RuntimeEntityAssets, material_flags: u32) -> RuntimeAssets {
    let base = short_models::world(entities.source_manifest_sha256(), 256, 255);
    let mut quads = base.model_quads().to_vec();
    for quad in &mut quads {
        quad.uvs.rotate_left(1);
    }
    let source_side = assets::TILE_SIZE * 2;
    let mut mips = Vec::new();
    let mut size = source_side;
    loop {
        mips.push(assets::TextureMip {
            size,
            rgba8: (0..size * size)
                .flat_map(|pixel| {
                    let [x, y] = [pixel % size, pixel / size];
                    [
                        (x * 3) as u8,
                        (y * 5) as u8,
                        (x + y) as u8,
                        if (x + y * 2).is_multiple_of(7) {
                            0
                        } else {
                            255
                        },
                    ]
                })
                .collect::<Vec<_>>()
                .into(),
        });
        if size == 1 {
            break;
        }
        size /= 2;
    }
    let overlay = assets::BlockOverlay {
        visuals: vec![assets::BlockVisual {
            native_material: assets::native_material::NativeMaterialFacts::Unresolved,
            faces: [1; 6],
            flags: assets::BlockFlags::OCCLUDES_FULL_FACE,
            kind: assets::VisualKind::Model,
            support: assets::VisualSupport::Exact,
            contributor_role: assets::ContributorRole::Primary,
            model_template: 0,
            animation: assets::NO_ANIMATION,
            variant: 0,
            destroy: assets::compiled::block_destroy::BlockDestroyFacts::Unresolved,
        }],
        light_properties: vec![assets::LightProperties::OPAQUE_DARK],
        materials: vec![
            assets::Material::unvaried(),
            assets::Material {
                texture: assets::TextureRef::new(1, 0).unwrap(),
                flags: material_flags,
                ..assets::Material::unvaried()
            },
        ],
        model_templates: base.model_templates().to_vec(),
        model_quads: quads,
        textures: vec![assets::TextureArray {
            layers: 1,
            mips: mips.into(),
        }],
        ..Default::default()
    };
    base.with_block_overlay(VISUAL.0, &overlay).unwrap()
}

/// Retains the item's model-thumbnail binding while omitting authored carried cube faces.
fn presentation(
    entities: &RuntimeEntityAssets,
    original: &assets::RuntimeIconCatalog,
) -> UiPresentationRuntime {
    let models: Vec<_> = original
        .entries()
        .iter()
        .map(|entry| assets::IconBlockModel {
            sprite: entry.sprite,
            visual: VISUAL,
        })
        .collect();
    let icons = assets::RuntimeIconCatalog::decode(
        &assets::encode_icon_catalog_with_blocks(
            entities.source_manifest_sha256(),
            original.sprites(),
            original.entries(),
            &[],
            &models,
        )
        .unwrap(),
    )
    .unwrap();
    UiPresentationRuntime::with_hud_and_icons(
        ui_engine::test_support::fixture_font(),
        crate::test_support::fixture_hud(),
        Arc::new(icons),
    )
    .unwrap()
}

#[test]
fn cutout_world_model_slots_preserve_original_texels_and_rotated_uvs() {
    let (base, entities) = cubes::catalog_for_visual(ITEM, 255, 255, VISUAL);
    let world = world(&entities, assets::MATERIAL_FLAG_ALPHA_CUTOUT);
    let block = world.resolve(assets::NetworkIdMode::Sequential, VISUAL.0);
    let material = block.face(assets::BlockFace::West).material_id();
    let (tile, side, _) = assets::gui_item::material_tile(world.materials(), material, |page| {
        world
            .texture_pages()
            .get(page as usize)
            .map(|page| &page.texture)
    })
    .unwrap();
    assert_eq!(side, (assets::TILE_SIZE * 2) as usize);
    let mut presentation = presentation(&entities, base.icon_catalog.as_deref().unwrap());
    presentation.set_gui_models(&world, &entities).unwrap();
    let icon = presentation.item_icon(ITEM, 0).unwrap();
    let actual = &presentation.gui_models.models[&icon_key(icon)];
    let source = presentation.gui_models.textures[&atlas::key([side as u16; 2], tile)];
    assert_eq!(
        [source.uv[2] - source.uv[0], source.uv[3] - source.uv[1]],
        [side as u16; 2]
    );
    let page = &presentation.textures.pages()[usize::from(source.page)];
    for y in 0..side {
        let target = ((usize::from(source.uv[1]) + y) * page.dimensions()[0] as usize
            + usize::from(source.uv[0]))
            * 4;
        assert_eq!(
            &page.pixels()[target..target + side * 4],
            &tile[y * side * 4..(y + 1) * side * 4]
        );
    }
    let first = (presentation.textures.dynamic_start() + MODEL_PAGE) as u16;
    let mut expected_atlas = atlas::Atlas::new(first, MODEL_PAGES);
    let expected = block_models::mesh(&world, VISUAL, &mut expected_atlas).unwrap();
    let (_, textures) = expected_atlas.finish().unwrap();
    let expected_source = textures[&atlas::key([side as u16; 2], tile)];
    assert_eq!(actual.indices(), expected.indices());
    for (actual, expected) in actual.vertices().iter().zip(expected.vertices()) {
        assert_eq!(actual.position, expected.position);
        assert_eq!(actual.clip_z, expected.clip_z);
        for axis in 0..2 {
            assert_eq!(
                actual.uv[axis] - f32::from(source.uv[axis]),
                expected.uv[axis] - f32::from(expected_source.uv[axis])
            );
        }
    }
}

#[test]
fn world_models_keep_unshaded_thumbnails_when_face_dimming_is_disabled() {
    let (base, entities) = cubes::catalog_for_visual(ITEM, 255, 255, VISUAL);
    for flags in [
        0,
        assets::MATERIAL_FLAG_DISABLE_AO,
        assets::MATERIAL_FLAG_DISABLE_FACE_DIMMING,
        assets::MATERIAL_FLAG_DISABLE_AO | assets::MATERIAL_FLAG_DISABLE_FACE_DIMMING,
    ] {
        let world = world(&entities, assets::MATERIAL_FLAG_ALPHA_CUTOUT | flags);
        let mut presentation = presentation(&entities, base.icon_catalog.as_deref().unwrap());
        presentation.set_gui_models(&world, &entities).unwrap();
        let icon = presentation
            .item_icon(ITEM, 0)
            .expect("thumbnail remains available");
        assert_eq!(
            presentation.gui_models.models.contains_key(&icon_key(icon)),
            flags & assets::MATERIAL_FLAG_DISABLE_FACE_DIMMING == 0,
            "a shaded mesh must not replace an unshaded thumbnail"
        );
    }
}
