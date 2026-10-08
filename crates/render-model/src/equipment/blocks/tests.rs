use super::*;
use assets::{ContributorRole, NO_MODEL_TEMPLATE, TextureRef};

fn overlay(flags: BlockFlags, material_flags: u32, size: u32) -> BlockOverlay {
    let mut mips = Vec::new();
    let mut side = size;
    loop {
        let mut pixels = vec![0; side as usize * side as usize * 4 * 2];
        for pixel in pixels.chunks_exact_mut(4) {
            pixel.copy_from_slice(&[40, 80, 120, 128]);
        }
        mips.push(TextureMip {
            size: side,
            rgba8: pixels.into(),
        });
        if side == 1 {
            break;
        }
        side /= 2;
    }
    BlockOverlay {
        visuals: vec![assets::BlockVisual {
            faces: [1; 6],
            flags,
            kind: VisualKind::Cube,
            support: VisualSupport::Exact,
            contributor_role: ContributorRole::Primary,
            model_template: NO_MODEL_TEMPLATE,
            animation: NO_ANIMATION,
            variant: 0,
        }],
        materials: vec![
            Material::unvaried(),
            Material {
                texture: TextureRef::new(1, 1).unwrap(),
                flags: material_flags,
                ..Material::unvaried()
            },
        ],
        texture: Some(TextureArray {
            layers: 2,
            mips: mips.into(),
        }),
        ..Default::default()
    }
}

#[test]
fn cube_sheets_ignore_gameplay_flags_and_terrain_occlusion() {
    for flags in [
        BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE | BlockFlags::FIRE_FLAMMABLE,
        BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE | BlockFlags::FIRE_TOP_SUPPORT,
        BlockFlags::CUBE_GEOMETRY,
        BlockFlags::CUBE_GEOMETRY | BlockFlags::LEAF_MODEL,
    ] {
        let sheet =
            overlay_sheet(&overlay(flags, 0, 16), 0).expect("cube shape determines held geometry");
        assert_eq!([sheet.width, sheet.height], assets::BLOCK_ITEM_SHEET_SIZE);
    }
}

#[test]
fn cube_sheets_preserve_blended_and_cutout_face_pixels() {
    for flags in [
        assets::MATERIAL_FLAG_ALPHA_BLEND,
        assets::MATERIAL_FLAG_ALPHA_CUTOUT,
    ] {
        let sheet = overlay_sheet(&overlay(BlockFlags::CUBE_GEOMETRY, flags, 32), 0)
            .expect("terrain alpha does not turn a held cube into a sprite");
        assert!(
            sheet
                .rgba8
                .chunks_exact(4)
                .all(|pixel| pixel == [40, 80, 120, 128])
        );
    }
}

#[test]
fn world_isotropy_preserves_carried_cube_face_pixels() {
    for flags in [
        0,
        assets::MATERIAL_FLAG_ALPHA_BLEND,
        assets::MATERIAL_FLAG_ALPHA_CUTOUT,
    ] {
        let mut source = overlay(
            BlockFlags::CUBE_GEOMETRY,
            flags,
            BLOCK_ITEM_FACE_SIDE.into(),
        );
        source.texture.as_mut().unwrap().mips[0].rgba8[TILE * TILE * 4] = 91;
        let expected = overlay_sheet(&source, 0).unwrap();
        source.materials[1].flags |= assets::MATERIAL_FLAG_ISOTROPIC;
        let actual = overlay_sheet(&source, 0)
            .expect("world-position rotation preserves carried face admission");
        assert_eq!(actual.rgba8, expected.rgba8);
        source.materials[1].flags |= assets::MATERIAL_FLAG_FOLIAGE_TINT;
        assert!(overlay_sheet(&source, 0).is_none());
    }
}

#[test]
fn unresolved_tint_and_non_cube_shapes_do_not_create_cube_sheets() {
    assert!(
        overlay_sheet(
            &overlay(
                BlockFlags::CUBE_GEOMETRY,
                assets::MATERIAL_FLAG_FOLIAGE_TINT,
                16
            ),
            0
        )
        .is_none()
    );
    let mut model = overlay(BlockFlags::empty(), 0, 16);
    model.visuals[0].kind = VisualKind::Model;
    assert!(overlay_sheet(&model, 0).is_none());
}

#[test]
fn transparent_cube_templates_keep_cube_geometry() {
    let mut ice = overlay(BlockFlags::empty(), assets::MATERIAL_FLAG_ALPHA_BLEND, 16);
    ice.visuals[0].kind = VisualKind::Model;
    ice.visuals[0].model_template = 0;
    ice.model_templates.push(assets::ModelTemplate {
        quad_start: 0,
        quad_count: 6,
        flags: assets::MODEL_TEMPLATE_FLAG_TRANSPARENT_CUBE,
    });
    let sheet = overlay_sheet(&ice, 0).expect("transparent cube is held as a block");
    assert_eq!(&sheet.rgba8[..4], &[40, 80, 120, 128]);
    ice.model_templates[0].flags = assets::MODEL_TEMPLATE_FLAG_STAIR;
    assert!(overlay_sheet(&ice, 0).is_none());
}

#[test]
fn pinned_cube_items_have_sheets_when_carriers_are_available() {
    let (Ok(world_path), Ok(entity_path)) = (
        std::env::var("PINNED_WORLD_CARRIER"),
        std::env::var("PINNED_ENTITY_CARRIER"),
    ) else {
        eprintln!("missing fixture: PINNED_WORLD_CARRIER and PINNED_ENTITY_CARRIER");
        return;
    };
    let world = RuntimeAssets::decode(&std::fs::read(world_path).unwrap()).unwrap();
    let entities = RuntimeEntityAssets::decode(&std::fs::read(entity_path).unwrap()).unwrap();
    let sheets = collect(&world, &entities);
    for name in [
        "black_wool",
        "white_wool",
        "ice",
        "glass",
        "oak_planks",
        "stone",
    ] {
        let identifier = format!("minecraft:{name}");
        let definition = entities
            .item_visuals()
            .iter()
            .find(|entry| entry.key.identifier.as_ref() == identifier)
            .unwrap();
        let ItemVisualDefinitionRoute::BlockItem { block_visual } = definition.route else {
            panic!("{identifier} has no block route");
        };
        assert!(
            sheets.by_visual.contains_key(&block_visual.0),
            "{identifier} must retain its held cube"
        );
    }
    eprintln!(
        "{} cube visuals share {} sheets",
        sheets.by_visual.len(),
        sheets.sheets.len()
    );
}
