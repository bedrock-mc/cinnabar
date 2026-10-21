use super::*;
use protocol::{CustomBlockVisuals, CustomVisualComponents};

/// A full-block furniture piece naming one texture on every face.
fn chair(name: &str, texture: &str) -> protocol::CustomBlock {
    block(
        name,
        1,
        CustomBlockVisuals {
            base: CustomVisualComponents {
                geometry: Some("minecraft:geometry.full_block".into()),
                materials: materials(texture),
                ..CustomVisualComponents::default()
            },
            ..CustomBlockVisuals::default()
        },
    )
}

/// Base carrier materials behind the vanilla keys: oak and lucky live on the
/// base page, so the overlay can borrow them when the server ships no raster.
/// Layer zero matches the diagnostic session base the tests apply onto.
fn base_materials() -> Vec<assets::Material> {
    vec![
        assets::Material::unvaried(),
        assets::Material {
            texture: assets::TextureRef::new(0, 0).unwrap(),
            ..assets::Material::unvaried()
        },
        assets::Material {
            texture: assets::TextureRef::new(0, 0).unwrap(),
            ..assets::Material::unvaried()
        },
    ]
}

/// Encodes static materials with enough base layers for every supplied reference.
fn base(materials: Vec<assets::Material>) -> RuntimeAssets {
    let diagnostic = RuntimeAssets::diagnostic();
    let layers = materials
        .iter()
        .map(|material| material.texture.layer() + 1)
        .max()
        .unwrap();
    let mut pages = diagnostic.texture_pages().to_vec();
    pages[0].texture.layers = layers;
    for mip in &mut pages[0].texture.mips {
        mip.rgba8 = mip.rgba8.repeat(layers as usize).into();
    }
    let compiled = assets::CompiledAssets {
        visuals: vec![super::super::diagnostic_visual()].into(),
        light_properties: vec![assets::LightProperties::OPAQUE_DARK].into(),
        hashed: Box::new([]),
        materials: materials.into(),
        model_templates: Box::new([]),
        model_quads: Box::new([]),
        animations: Box::new([]),
        animation_frames: Box::new([]),
        texture_pages: pages.into(),
        biomes: assets::CompiledBiomeAssets::diagnostic(),
        provenance: assets::BlobProvenance {
            source_manifest_sha256: [1; 32],
            block_registry_sha256: [2; 32],
            light_registry_sha256: [3; 32],
            biome_registry_sha256: [4; 32],
        },
    };
    RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap()
}

/// Keys for the base oak texture, absent from the server fixture.
fn oak_keys() -> assets::material_keys::MaterialKeys {
    assets::material_keys::MaterialKeys::from_entries([(1, "oak_planks")])
        .with_aliases([("oak_planks", "textures/blocks/oak_planks")])
}

/// Base keys for both an absent raster and one shipped by the server fixture.
fn keys() -> assets::material_keys::MaterialKeys {
    assets::material_keys::MaterialKeys::from_entries([(1, "oak_planks"), (2, "lucky")])
        .with_aliases([
            ("oak_planks", "textures/blocks/oak_planks"),
            ("lucky", "textures/blocks/lucky"),
        ])
}

/// Compiles custom blocks with matching base materials and texture keys.
fn compile(
    blocks: &CustomBlocks,
    keys: Option<&assets::material_keys::MaterialKeys>,
) -> super::super::CompiledBlockOverlay {
    let base = base(base_materials());
    super::super::compile_block_overlay(
        &view(),
        blocks,
        false,
        keys,
        assets::MAX_TEXTURE_LAYERS as u32,
        Some(&base),
    )
    .expect("overlay")
}

// Furniture naming vanilla planks the server stack does not ship renders
// from the base page instead of degrading to the diagnostic look.
#[test]
fn vanilla_faces_reuse_the_base_page_without_missing_textures() {
    let keys = oak_keys();
    let blocks = CustomBlocks {
        blocks: vec![chair("test:chair", "oak_planks")].into(),
        ..Default::default()
    };
    let compiled = compile(&blocks, Some(&keys));
    assert_eq!(compiled.gaps.missing_textures, 0);
    assert_eq!(compiled.overlay.visuals.len(), 1);
    let visual = compiled.overlay.visuals[0];
    assert_eq!(visual.kind, VisualKind::Cube);
    let material = compiled.overlay.materials[visual.faces[BlockFace::Up as usize] as usize];
    assert_eq!(
        (material.texture.page(), material.texture.layer()),
        (0, 0),
        "the chair face reuses the base oak layer"
    );
    assert_eq!(
        compiled.overlay.material_overrides.len(),
        0,
        "the vanilla fallback itself claims no override"
    );
    let session = RuntimeAssets::diagnostic()
        .with_block_overlay(1, &compiled.overlay)
        .expect("base-page materials apply");
    let admitted = session.resolve(NetworkIdMode::Sequential, 1);
    assert!(admitted.is_known());
    assert_eq!(admitted.kind(), VisualKind::Cube);
}

#[test]
fn absent_pack_faces_reuse_the_base_page_after_every_overlay_shard_is_full() {
    let view = view_with_geometry(
        br#"{"format_version":"1.12.0","minecraft:geometry":[{
            "description":{"identifier":"geometry.floor","texture_width":16,"texture_height":16},
            "bones":[{"name":"floor","cubes":[{"origin":[-8,0,-8],"size":[16,1,16],"uv":[0,0]}]}]
        }]}"#,
    );
    let mut materials = base_materials();
    materials[1].texture = assets::TextureRef::new(0, 1).unwrap();
    let base = base(materials);
    let keys = keys();
    for geometry in [super::super::FULL_BLOCK, "geometry.floor"] {
        let mut floor = chair("test:floor", "oak_planks");
        Arc::make_mut(&mut floor.visual).base.geometry = Some(geometry.into());
        let blocks = CustomBlocks {
            blocks: vec![
                chair("test:static", "lucky"),
                chair("test:animated", "gen"),
                floor,
            ]
            .into(),
            ..Default::default()
        };
        let compiled =
            super::super::compile_block_overlay(&view, &blocks, false, Some(&keys), 1, Some(&base))
                .unwrap();
        assert_eq!(
            compiled.overlay.textures.len(),
            super::super::MAX_OVERLAY_SHARDS
        );
        assert!(
            compiled
                .overlay
                .textures
                .iter()
                .all(|shard| shard.layers == 1)
        );
        assert_eq!(compiled.gaps.missing_textures, 0);
        assert_eq!(
            compiled.overlay.visuals[2].kind,
            if geometry == super::super::FULL_BLOCK {
                VisualKind::Cube
            } else {
                VisualKind::Model
            }
        );
        let session = base.with_block_overlay(1, &compiled.overlay).unwrap();
        let visual = session.resolve(NetworkIdMode::Sequential, 3);
        for face in BlockFace::ALL {
            let material = session.material(visual.face(face).material_id());
            assert_eq!(material.texture, base.material(1).texture);
            assert_eq!(material.animation, assets::NO_ANIMATION);
        }
        if let Some(template) = visual.model_template() {
            let template = session.model_templates()[template as usize];
            let start = template.quad_start as usize;
            for quad in &session.model_quads()[start..start + template.quad_count as usize] {
                assert_eq!(
                    session.material(quad.material).texture,
                    base.material(1).texture
                );
            }
        }
    }
}

// Without base materials the same chair degrades per face instead of
// sinking the whole cube.
#[test]
fn vanilla_faces_without_a_base_degrade_per_face() {
    let blocks = CustomBlocks {
        blocks: vec![chair("test:chair", "oak_planks")].into(),
        ..Default::default()
    };
    let compiled = super::compile_block_overlay(&view(), &blocks, false, None).expect("overlay");
    assert_eq!(compiled.overlay.visuals[0].kind, VisualKind::Cube);
    assert!(
        compiled.overlay.visuals[0]
            .faces
            .iter()
            .all(|&face| face == assets::DIAGNOSTIC_MATERIAL)
    );
    assert_eq!(compiled.gaps.missing_textures, 1);
}

// A server raster wins over the base page for the same key.
#[test]
fn server_rasters_win_over_the_base_page() {
    let keys = keys();
    let blocks = CustomBlocks {
        blocks: vec![chair("test:lucky_chair", "lucky")].into(),
        ..Default::default()
    };
    let compiled = compile(&blocks, Some(&keys));
    assert_eq!(compiled.gaps.missing_textures, 0);
    let visual = compiled.overlay.visuals[0];
    let material = compiled.overlay.materials[visual.faces[BlockFace::Up as usize] as usize];
    assert_eq!(
        material.texture.page(),
        1,
        "the shipped server raster stays on the overlay page"
    );
    assert_eq!(
        compiled.overlay.material_overrides.len(),
        1,
        "only the shipped lucky raster repoints a base material"
    );
    let replacement = &compiled.overlay.material_overrides[0];
    assert_eq!(replacement.material, 2);
    assert_eq!(replacement.texture.page(), 1);
}

// Keys the base carrier never compiled degrade per face instead of borrowing.
#[test]
fn unknown_keys_degrade_per_face_with_a_base() {
    let keys = keys();
    let blocks = CustomBlocks {
        blocks: vec![chair("test:stranger", "absent")].into(),
        ..Default::default()
    };
    let compiled = compile(&blocks, Some(&keys));
    assert_eq!(compiled.overlay.visuals[0].kind, VisualKind::Cube);
    assert!(
        compiled.overlay.visuals[0]
            .faces
            .iter()
            .all(|&face| face == assets::DIAGNOSTIC_MATERIAL)
    );
    assert_eq!(compiled.gaps.missing_textures, 1);
}

#[test]
fn custom_fallback_uses_the_first_catalog_variant_after_sidecar_roundtrip() {
    let mut materials = base_materials();
    materials[1].texture = assets::TextureRef::new(0, 1).unwrap();
    materials[2].texture = assets::TextureRef::new(0, 2).unwrap();
    let base = base(materials);
    let keys = assets::material_keys::MaterialKeys::from_entries([(1, "farmland"), (2, "farmland")])
        .with_aliases([("farmland", "textures/blocks/farmland_wet")])
        .with_flipbook_selectors([(1, (1, 0)), (2, (0, 0))])
        .with_sources(
            [("farmland".into(), serde_json::json!({"textures": ["textures/blocks/farmland_wet", "textures/blocks/farmland_dry"]}))],
            [],
            [],
        );
    let keys = assets::material_keys::MaterialKeys::from_json(
        &keys.to_json(base.materials().len() as u32),
        base.materials().len(),
    )
    .unwrap();
    let blocks = CustomBlocks {
        blocks: vec![chair("test:farmland_chair", "farmland")].into(),
        ..Default::default()
    };
    let compiled = super::super::compile_block_overlay(
        &view(),
        &blocks,
        false,
        Some(&keys),
        assets::MAX_TEXTURE_LAYERS as u32,
        Some(&base),
    )
    .unwrap();
    assert_eq!(compiled.gaps.missing_textures, 0);
    assert_eq!(compiled.overlay.visuals[0].kind, VisualKind::Cube);
    let face = compiled.overlay.visuals[0].faces[BlockFace::Up as usize];
    assert_eq!(
        compiled.overlay.materials[face as usize].texture,
        base.material(2).texture
    );
    assert_eq!(compiled.overlay.textures.first().unwrap().layers, 1);
    assert!(compiled.overlay.material_overrides.is_empty());
    let session = base.with_block_overlay(1, &compiled.overlay).unwrap();
    let face = session
        .resolve(NetworkIdMode::Sequential, 1)
        .face(BlockFace::Up);
    assert_eq!(
        session.material(face.material_id()).texture,
        base.material(2).texture
    );
    assert_eq!(session.material(1), base.material(1));
    assert_eq!(session.material(2), base.material(2));
}

#[test]
fn custom_catalog_redirects_resolve_the_selected_pack_or_carrier_path() {
    use assets::material_keys::{MaterialKeys, MaterialTextureSource};
    use serde_json::json;

    let mut materials = base_materials();
    materials[1].texture = assets::TextureRef::new(0, 1).unwrap();
    materials[2].texture = assets::TextureRef::new(0, 2).unwrap();
    let base = base(materials);
    let keys = MaterialKeys::from_entries([(1, "oak_planks"), (2, "birch_planks")])
        .with_aliases([
            ("oak_planks", "textures/blocks/oak_planks"),
            ("birch_planks", "textures/blocks/birch_planks"),
        ])
        .with_sources(
            [],
            [(1, "oak_planks"), (2, "birch_planks")].map(|(id, key)| {
                let path = format!("textures/blocks/{key}");
                (
                    id,
                    MaterialTextureSource {
                        key: key.into(),
                        variant: 0,
                        path: path.clone().into(),
                        raster: format!("{path}.png").into(),
                        baked_tint: None,
                    },
                )
            }),
            [],
        );
    let blocks = CustomBlocks {
        blocks: vec![chair("test:chair", "oak_planks")].into(),
        ..Default::default()
    };
    for (path, expected_missing, replaces_base) in [
        ("textures/blocks/birch_planks", false, true),
        ("textures/blocks/absent", true, false),
        ("textures/blocks/oak_planks", false, false),
        ("textures/blocks/lucky", false, true),
    ] {
        let terrain = json!({"texture_data":{"oak_planks":{"textures":path}}});
        let view = view_with_catalog(GEOMETRY.as_bytes(), &terrain.to_string());
        let compiled = super::super::compile_block_overlay(
            &view,
            &blocks,
            false,
            Some(&keys),
            assets::MAX_TEXTURE_LAYERS as u32,
            Some(&base),
        )
        .unwrap();
        // Declined fallbacks still render their cube; failed faces keep the
        // diagnostic material instead of sinking the visual.
        assert_eq!(compiled.overlay.visuals[0].kind, VisualKind::Cube, "{path}");
        assert!(
            compiled.overlay.visuals[0]
                .faces
                .iter()
                .all(|&face| (face == assets::DIAGNOSTIC_MATERIAL) == expected_missing),
            "{path}"
        );
        assert_eq!(compiled.gaps.missing_textures, u32::from(expected_missing));
        assert_eq!(
            compiled
                .overlay
                .material_overrides
                .iter()
                .any(|replacement| replacement.material == 1),
            replaces_base
        );
        let session = base.with_block_overlay(1, &compiled.overlay).unwrap();
        assert_eq!(
            session.resolve(NetworkIdMode::Sequential, 1).kind(),
            VisualKind::Cube
        );
        assert_eq!(session.material(2), base.material(2));
        if replaces_base {
            assert_eq!(session.material(1).texture.page(), 1);
        }
    }
}

#[test]
fn custom_fallback_preserves_catalog_tint_or_stays_diagnostic() {
    use assets::material_keys::{MaterialKeys, MaterialTextureSource};
    use serde_json::json;

    let tint = [32_u8, 128, 48];
    let mut materials = base_materials();
    materials[1].texture = assets::TextureRef::new(0, 1).unwrap();
    materials[2].texture = assets::TextureRef::new(0, 2).unwrap();
    let base = base(materials);
    let blocks = CustomBlocks {
        blocks: vec![chair("test:lily_chair", "waterlily")].into(),
        ..Default::default()
    };
    for copied_tint in [Some(tint), None, Some([48, 128, 32])] {
        let keys = MaterialKeys::from_entries([(1, "waterlily"), (2, "waterlily")])
            .with_aliases([("waterlily", "textures/blocks/waterlily")])
            .with_fixed_tints([("waterlily", tint)])
            .with_sources(
                [("waterlily".into(), json!({"textures":{"path":"textures/blocks/waterlily", "tint_color":format!("#{:02x}{:02x}{:02x}", tint[0], tint[1], tint[2])}}))],
                [(1, None), (2, copied_tint)].map(|(id, baked_tint)| (id, MaterialTextureSource {
                    key: "waterlily".into(),
                    variant: 0,
                    path: "textures/blocks/waterlily".into(),
                    raster: "textures/blocks/waterlily.png".into(),
                    baked_tint,
                })),
                [],
            );
        let keys = MaterialKeys::from_json(
            &keys.to_json(base.materials().len() as u32),
            base.materials().len(),
        )
        .unwrap();
        let compiled = super::super::compile_block_overlay(
            &view(),
            &blocks,
            false,
            Some(&keys),
            assets::MAX_TEXTURE_LAYERS as u32,
            Some(&base),
        )
        .unwrap();
        let matched = copied_tint == Some(tint);
        assert_eq!(compiled.overlay.visuals[0].kind, VisualKind::Cube);
        assert!(
            compiled.overlay.visuals[0]
                .faces
                .iter()
                .all(|&face| (face == assets::DIAGNOSTIC_MATERIAL) == !matched)
        );
        assert_eq!(compiled.gaps.missing_textures, u32::from(!matched));
        assert_eq!(compiled.overlay.textures.first().unwrap().layers, 1);
        assert!(compiled.overlay.material_overrides.is_empty());
        let session = base.with_block_overlay(1, &compiled.overlay).unwrap();
        let block = session.resolve(NetworkIdMode::Sequential, 1);
        assert_eq!(block.kind(), VisualKind::Cube);
        if matched {
            assert_eq!(
                session
                    .material(block.face(BlockFace::Up).material_id())
                    .texture,
                base.material(2).texture
            );
        }
        assert_eq!(session.material(1), base.material(1));
        assert_eq!(session.material(2), base.material(2));
    }
}

/// Custom terrain keys can name built-in raster files without shipping their own copies.
#[test]
fn custom_texture_aliases_resolve_carrier_pixels_for_blocks_and_items() {
    let keys = oak_keys().with_sources(
        [],
        [(
            1,
            assets::material_keys::MaterialTextureSource {
                key: "oak_planks".into(),
                variant: 0,
                path: "textures/blocks/oak_planks".into(),
                raster: "textures/blocks/oak_planks.png".into(),
                baked_tint: None,
            },
        )],
        [],
    );
    let view = view_with_catalog(
        GEOMETRY.as_bytes(),
        r#"{"texture_data":{"fixture:wood":{"textures":"textures/blocks/oak_planks"}}}"#,
    );
    let base = base(base_materials());
    let blocks = CustomBlocks {
        blocks: vec![chair("fixture:chair", "fixture:wood")].into(),
        ..Default::default()
    };
    for hashed in [false, true] {
        let compiled = super::super::compile_block_overlay(
            &view,
            &blocks,
            hashed,
            Some(&keys),
            assets::MAX_TEXTURE_LAYERS as u32,
            Some(&base),
        )
        .unwrap();
        assert_eq!(compiled.gaps.missing_textures, 0);
        let icons = crate::runtime::network::item_icons::custom_block_icons(
            &compiled.overlay,
            &blocks,
            hashed,
            &[("fixture:chair".into(), "fixture:chair".into())],
            None,
        );
        assert_eq!(icons.icons.len(), 1);
        assert_eq!(icons.block_sheets.len(), 1);
        assert!(icons.misses.is_empty());
    }
}
