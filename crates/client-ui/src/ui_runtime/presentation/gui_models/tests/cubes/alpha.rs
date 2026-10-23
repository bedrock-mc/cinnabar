use super::*;
use assets::{
    BlockFace, BlockOverlay, BlockVisual, Material, TextureArray, TextureMip, TextureRef,
};

/// Binds the fixture cube's six faces to the requested material modes without installed assets.
pub(super) fn world(flags: [u32; 6]) -> RuntimeAssets {
    let mut mips = Vec::new();
    let mut side = assets::TILE_SIZE;
    loop {
        mips.push(TextureMip {
            size: side,
            rgba8: [20, 40, 60, 255].repeat((side * side) as usize).into(),
        });
        if side == 1 {
            break;
        }
        side /= 2;
    }
    let mut materials = vec![Material::unvaried()];
    materials.extend(flags.map(|flags| Material {
        texture: TextureRef::new(1, 0).unwrap(),
        flags,
        ..Material::unvaried()
    }));
    let overlay = BlockOverlay {
        visuals: vec![BlockVisual {
            native_material: assets::native_material::NativeMaterialFacts::Unresolved,
            faces: BlockFace::ALL.map(|face| face as u32 + 1),
            flags: assets::BlockFlags::CUBE_GEOMETRY,
            kind: assets::VisualKind::Cube,
            support: assets::VisualSupport::Exact,
            contributor_role: assets::ContributorRole::Primary,
            model_template: assets::NO_MODEL_TEMPLATE,
            animation: assets::NO_ANIMATION,
            variant: 0,
            destroy: assets::compiled::block_destroy::BlockDestroyFacts::Unresolved,
        }],
        light_properties: vec![assets::LightProperties::OPAQUE_DARK],
        materials,
        textures: vec![TextureArray {
            layers: 1,
            mips: mips.into(),
        }],
        ..Default::default()
    };
    RuntimeAssets::diagnostic()
        .with_block_overlay(VISUAL.0, &overlay)
        .unwrap()
}

#[test]
fn carried_cubes_keep_unshaded_thumbnails_when_face_dimming_is_disabled() {
    for face in BlockFace::ALL {
        let mut flags = [0; 6];
        flags[face as usize] = assets::MATERIAL_FLAG_DISABLE_FACE_DIMMING;
        let (mut presentation, entities) = catalog_for(ITEM, 255, 255);
        presentation
            .set_gui_models(&world(flags), &entities)
            .unwrap();
        let icon = presentation
            .item_icon(ITEM, 0)
            .expect("thumbnail remains available");
        assert!(
            !presentation
                .gui_models
                .sources
                .models
                .contains_key(&icon_key(icon))
        );
        let mut nodes = [control(icon, 255, false)];
        let original = nodes.clone();
        presentation.apply_gui_models(&mut nodes);
        assert_eq!(nodes, original, "the slot retains its baked thumbnail");
    }
}

#[test]
fn fractional_non_blended_texels_are_thresholded_before_mesh_sampling() {
    let threshold = assets::gui_item::GUI_BLOCK_ALPHA_THRESHOLD;
    for flags in [0, assets::MATERIAL_FLAG_ALPHA_CUTOUT] {
        for alpha in [threshold - 1, threshold, threshold + 1] {
            let accepted = if alpha >= threshold { 255 } else { 0 };
            let (mut presentation, entities) = catalog_for(ITEM, alpha, accepted);
            presentation
                .set_gui_models(&world([flags; 6]), &entities)
                .unwrap();
            let icons = presentation.icon_catalog.as_deref().unwrap();
            let sheet = &icons.sprites()[icons.block_sheets()[0].sprite as usize];
            let mut expected = sheet.rgba8.to_vec();
            for texel in expected.as_chunks_mut::<4>().0 {
                texel[3] = if texel[3] >= threshold { 255 } else { 0 };
            }
            let source = presentation.gui_models.sources.textures
                [&super::super::super::atlas::key([sheet.width, sheet.height], &expected)];
            let page = &presentation.textures.pages()[usize::from(source.page)];
            for y in 0..usize::from(sheet.height) {
                let width = usize::from(sheet.width);
                let original = y * width * 4;
                let target = ((usize::from(source.uv[1]) + y) * page.dimensions()[0] as usize
                    + usize::from(source.uv[0]))
                    * 4;
                assert_eq!(
                    &page.pixels()[target..target + width * 4],
                    &expected[original..original + width * 4]
                );
            }
            for entry in icons.entries() {
                let icon = presentation
                    .item_icon(&entry.identifier, entry.metadata)
                    .unwrap();
                let mesh = &presentation.gui_models.sources.models[&icon_key(icon)];
                assert_eq!(mesh.indices().len(), assets::gui_item::CUBE_FACES.len() * 6);
                assert!(mesh.vertices().iter().all(|vertex| vertex.alpha_test));
            }
        }
    }
}

#[test]
fn fractional_blended_cube_faces_keep_the_composited_thumbnail_under_pressure() {
    for blended_faces in [
        [assets::MATERIAL_FLAG_ALPHA_BLEND; 6],
        BlockFace::ALL.map(|face| {
            if face == BlockFace::Up {
                assets::MATERIAL_FLAG_ALPHA_BLEND
            } else {
                assets::MATERIAL_FLAG_ALPHA_CUTOUT
            }
        }),
    ] {
        for pages in [0, MODEL_PAGES] {
            // Two alpha-128 layers in the composed thumbnail produce alpha 192.
            let (mut presentation, entities) = catalog_for(ITEM, 128, 192);
            presentation.set_equipment_catalog(Some(equipment(&entities, pages)));
            presentation
                .set_gui_models(&world(blended_faces), &entities)
                .unwrap();
            assert_eq!(presentation.gui_models.sources.pages.len(), pages);
            for entry in presentation.icon_catalog.as_deref().unwrap().entries() {
                let icon = presentation
                    .item_icon(&entry.identifier, entry.metadata)
                    .unwrap();
                let page = &presentation.textures.pages()[usize::from(icon.page)];
                let texel = (usize::from(icon.uv[1]) * page.dimensions()[0] as usize
                    + usize::from(icon.uv[0]))
                    * 4;
                assert_eq!(page.pixels()[texel + 3], 192);
                for alpha in [255, 254, 127, 0] {
                    for glint in [false, true] {
                        let original = control(icon, alpha, glint);
                        let mut nodes = [original.clone()];
                        presentation.apply_gui_models(&mut nodes);
                        assert_eq!(
                            nodes[0], original,
                            "blended faces keep their composed coverage"
                        );
                    }
                }
            }
        }
    }
}
