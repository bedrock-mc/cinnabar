use assets::{
    BlockFlags, BlockOverlay, BlockVisual, ContributorRole, MATERIAL_FLAG_ALPHA_CUTOUT,
    MATERIAL_FLAG_DISABLE_AO, Material, ModelQuad, ModelTemplate, TextureArray, TextureMip,
    TextureRef, VisualKind, VisualSupport,
};

/// A server geometry block of one full-size top quad whose material carries `flags`.
fn geometry_block(flags: u32) -> BlockOverlay {
    BlockOverlay {
        visuals: vec![BlockVisual {
            native_material: assets::native_material::NativeMaterialFacts::Unresolved,
            faces: [1; 6],
            flags: BlockFlags::empty(),
            kind: VisualKind::Model,
            support: VisualSupport::VanillaFallback,
            contributor_role: ContributorRole::Primary,
            model_template: 0,
            animation: assets::NO_ANIMATION,
            variant: 0,
            destroy: assets::compiled::block_destroy::BlockDestroyFacts::Unresolved,
        }],
        materials: vec![
            Material::unvaried(),
            Material {
                texture: TextureRef::new(1, 0).unwrap(),
                flags,
                ..Material::unvaried()
            },
        ],
        model_templates: vec![ModelTemplate {
            quad_start: 0,
            quad_count: 1,
            flags: 0,
        }],
        model_quads: vec![ModelQuad {
            positions: [[0, 256, 0], [0, 256, 256], [256, 256, 256], [256, 256, 0]],
            uvs: [[0, 0], [0, 4096], [4096, 4096], [4096, 0]],
            material: 1,
            flags: 0,
        }],
        textures: vec![TextureArray {
            layers: 1,
            mips: [TextureMip {
                size: 16,
                rgba8: vec![200; 16 * 16 * 4].into(),
            }]
            .into(),
        }],
        ..Default::default()
    }
}

// An alpha-tested server block has ambient occlusion off, which a thumbnail never draws, so its
// item still gets one.
#[test]
fn geometry_blocks_without_ambient_occlusion_get_thumbnails() {
    for flags in [
        MATERIAL_FLAG_ALPHA_CUTOUT,
        MATERIAL_FLAG_ALPHA_CUTOUT | MATERIAL_FLAG_DISABLE_AO,
        MATERIAL_FLAG_ALPHA_CUTOUT | assets::MATERIAL_FLAG_DISABLE_FACE_DIMMING,
        MATERIAL_FLAG_ALPHA_CUTOUT
            | MATERIAL_FLAG_DISABLE_AO
            | assets::MATERIAL_FLAG_DISABLE_FACE_DIMMING,
    ] {
        let sprite = pack_entity_compiler::overlay_block_icon(&geometry_block(flags), 0, None)
            .unwrap_or_else(|| panic!("no thumbnail for material flags {flags:#x}"));
        assert!(
            sprite
                .rgba8
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] != 0),
            "flags {flags:#x} drew nothing"
        );
    }
}

#[test]
fn carried_cube_icons_sample_the_referenced_overlay_shard() {
    for base_pages in [1, 2] {
        let mut overlay = geometry_block(0);
        overlay.base_texture_pages = base_pages;
        let mut second = overlay.textures[0].clone();
        second.mips[0].rgba8 = [50, 160, 220, 255].repeat(16 * 16).into();
        overlay.textures.push(second.clone());
        overlay.materials[1].texture = TextureRef::new(base_pages + 1, 0).unwrap();
        let actual = pack_entity_compiler::overlay_cube_icon(&overlay, [1; 6], None).unwrap();
        overlay.textures = vec![second];
        overlay.materials[1].texture = TextureRef::new(base_pages, 0).unwrap();
        let expected = pack_entity_compiler::overlay_cube_icon(&overlay, [1; 6], None).unwrap();
        assert_eq!(actual.rgba8, expected.rgba8);
        assert!(
            actual
                .rgba8
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] != 0)
        );
    }
}

#[test]
fn disabled_face_dimming_keeps_inventory_faces_unshaded() {
    let mut overlay = geometry_block(assets::MATERIAL_FLAG_DISABLE_FACE_DIMMING);
    overlay.model_quads[0].positions = [[0, 0, 0], [0, 256, 0], [256, 256, 0], [256, 0, 0]];
    let sprite = pack_entity_compiler::overlay_block_icon(&overlay, 0, None).unwrap();
    let visible = sprite
        .rgba8
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[3] != 0)
        .collect::<Vec<_>>();
    assert!(!visible.is_empty());
    assert!(visible.iter().all(|pixel| pixel[..3] == [200, 200, 200]));
    overlay.materials[1].flags = 0;
    let shaded = pack_entity_compiler::overlay_block_icon(&overlay, 0, None).unwrap();
    assert!(
        shaded
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[3] != 0 && pixel[0] < 200)
    );
}

#[test]
fn explicit_inventory_tints_preserve_texels_alpha_and_material_rejection() {
    let flags = assets::MATERIAL_FLAG_GRASS_TINT | assets::MATERIAL_FLAG_ALPHA_BLEND;
    let mut overlay = geometry_block(flags);
    assert!(pack_entity_compiler::overlay_block_icon(&overlay, 0, None).is_none());
    let resolve = |flags| {
        (flags & assets::MATERIAL_FLAG_TINT_MASK == assets::MATERIAL_FLAG_GRASS_TINT)
            .then_some([80, 180, 120])
    };
    let tinted = pack_entity_compiler::overlay_block_icon_with_tint(&overlay, 0, None, &resolve)
        .expect("a resolved tint is drawable");
    overlay.materials[1].flags &= !assets::MATERIAL_FLAG_TINT_MASK;
    pack_compile_input::apply_atlas_tint(&mut overlay.textures[0].mips[0].rgba8, [80, 180, 120]);
    let expected = pack_entity_compiler::overlay_block_icon(&overlay, 0, None).unwrap();
    assert_eq!(tinted.rgba8, expected.rgba8);
    assert!(
        tinted
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[3] == 200)
    );
    overlay.materials[1].flags |=
        assets::MATERIAL_FLAG_GRASS_TINT | assets::MATERIAL_FLAG_ROTATE_UV;
    assert!(
        pack_entity_compiler::overlay_block_icon_with_tint(&overlay, 0, None, &resolve).is_none()
    );
    overlay.materials[1].flags = assets::MATERIAL_FLAG_FOLIAGE_TINT;
    assert!(
        pack_entity_compiler::overlay_block_icon_with_tint(&overlay, 0, None, &resolve).is_none()
    );
}
