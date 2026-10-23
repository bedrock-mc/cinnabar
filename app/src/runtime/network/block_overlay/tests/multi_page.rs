use super::*;
use assets::{Material, TextureRef};

/// Compiles both a server raster and a borrowed texture against a 1,600-layer base carrier.
fn compile() -> (Arc<RuntimeAssets>, super::super::CompiledBlockOverlay) {
    let base = Arc::new(vanilla_fallback::base(vec![
        Material::unvaried(),
        Material {
            texture: TextureRef::new(1, 575).unwrap(),
            ..Material::unvaried()
        },
        Material {
            texture: TextureRef::new(1, 574).unwrap(),
            ..Material::unvaried()
        },
        Material {
            texture: TextureRef::new(0, assets::MAX_CARRIER_TEXTURE_LAYERS as u32 - 1).unwrap(),
            ..Material::unvaried()
        },
    ]));
    let keys = vanilla_fallback::keys();
    let blocks = CustomBlocks {
        blocks: ["lucky", "oak_planks"]
            .into_iter()
            .enumerate()
            .map(|(index, texture)| {
                block(
                    &format!("test:block{index}"),
                    1,
                    CustomBlockVisuals {
                        base: CustomVisualComponents {
                            geometry: Some("minecraft:geometry.full_block".into()),
                            materials: materials(texture),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                )
            })
            .collect(),
        ..Default::default()
    };
    let compiled = super::super::compile_block_overlay(
        &view(),
        &blocks,
        false,
        Some(&keys),
        assets::MAX_CARRIER_TEXTURE_LAYERS as u32,
        Some(&base),
    )
    .unwrap();
    (base, compiled)
}

#[test]
fn multi_page_base_applies_server_and_borrowed_textures_to_world_and_items() {
    let (base, compiled) = compile();
    assert_eq!(
        base.texture_pages()
            .iter()
            .map(|page| page.texture.layers)
            .collect::<Vec<_>>(),
        [assets::MAX_CARRIER_TEXTURE_LAYERS as u32, 576]
    );
    assert_eq!(compiled.overlay.base_texture_pages, 2);
    assert_eq!(compiled.gaps.missing_textures, 0);
    assert_eq!(compiled.overlay.material_overrides.len(), 1);
    assert_eq!(compiled.overlay.material_overrides[0].material, 2);
    assert_eq!(compiled.overlay.material_overrides[0].texture.page(), 2);
    // The same representation persists through the compiled resource-pack cache.
    let compiled: super::super::CompiledBlockOverlay =
        serde_json::from_slice(&serde_json::to_vec(&compiled).unwrap()).unwrap();
    let session = base.with_block_overlay(1, &compiled.overlay).unwrap();
    for (index, expected_page) in [2, 1].into_iter().enumerate() {
        let visual = compiled.overlay.visuals[index];
        let admitted = session.resolve(NetworkIdMode::Sequential, index as u32 + 1);
        let material = session.material(admitted.face(BlockFace::Up).material_id());
        assert_eq!(material.texture.page(), expected_page);
        assert!(
            pack_entity_compiler::overlay_block_icon(&compiled.overlay, index, Some(&base))
                .is_some()
        );
        assert!(
            render_model::equipment::blocks::overlay_face_sheet(
                &compiled.overlay,
                visual.faces,
                Some(&base)
            )
            .is_some()
        );
    }
    let replacement = compiled.overlay.material_overrides[0];
    assert_eq!(
        session.material(replacement.material).texture,
        replacement.texture
    );
}

#[test]
fn multi_page_session_reloads_restores_and_rejects_overflow_without_losing_base() {
    let (base, compiled) = compile();
    let compiled = Arc::new(compiled);
    let ids = 1..3;
    let install = |compiled| {
        super::super::super::resource_packs::session_runtime_assets(&base, Some(&ids), compiled)
    };
    let first = install(Some(&compiled));
    assert!(!Arc::ptr_eq(&first, &base));
    let mut changed = (*compiled).clone();
    let page = &mut changed.overlay.textures[0];
    for mip in &mut page.mips {
        mip.rgba8 = [12, 34, 56, 255].repeat(mip.rgba8.len() / 4).into();
    }
    let changed_overlay = Arc::new(changed.clone());
    let reloaded = install(Some(&changed_overlay));
    assert!(!Arc::ptr_eq(&first, &reloaded));
    assert_eq!(
        &reloaded.texture_pages()[2].texture.mips[0].rgba8[..4],
        &[12, 34, 56, 255]
    );
    assert!(Arc::ptr_eq(&install(None), &base));
    changed.overlay.textures.resize(
        assets::MAX_TEXTURE_PAGES - base.texture_pages().len() + 1,
        changed.overlay.textures[0].clone(),
    );
    assert!(Arc::ptr_eq(&install(Some(&Arc::new(changed))), &base));
}
