use super::{tests::cube_overlay, *};
use crate::{BlockFace, NetworkIdMode, TextureMip};

/// Builds small solid-color mips with a distinct color for every layer.
fn texture(layers: u32, color: u8) -> TextureArray {
    TextureArray {
        layers,
        mips: [16, 8, 4, 2, 1]
            .into_iter()
            .map(|size| TextureMip {
                size,
                rgba8: (0..layers)
                    .flat_map(|layer| {
                        std::iter::repeat_n([color, layer as u8, 0, 255], (size * size) as usize)
                            .flatten()
                    })
                    .collect(),
            })
            .collect(),
    }
}

/// Creates a 1,600-layer carrier split at the shipped page budget.
fn base() -> RuntimeAssets {
    let mut base = RuntimeAssets::diagnostic();
    base.texture_pages = vec![
        TexturePage::new(texture(crate::MAX_CARRIER_TEXTURE_LAYERS as u32, 10)),
        TexturePage::new(texture(
            1_600 - crate::MAX_CARRIER_TEXTURE_LAYERS as u32,
            20,
        )),
    ]
    .into();
    base.materials = vec![
        Material::unvaried(),
        Material {
            texture: TextureRef::new(0, 1).unwrap(),
            ..Material::unvaried()
        },
        Material {
            texture: TextureRef::new(1, 575).unwrap(),
            ..Material::unvaried()
        },
    ]
    .into();
    base
}

#[test]
fn multi_page_base_keeps_overrides_visuals_and_source_metadata_addressed() {
    let base = base();
    let keys = MaterialKeys::from_entries([(2, "test:second_page")]);
    assert_eq!(
        VanillaFallback::new(&keys, base.materials()).texture("test:second_page", None),
        Some(base.material(2).texture)
    );
    let mut overlay = cube_overlay(texture(1, 30));
    overlay.base_texture_pages = 2;
    overlay.textures.push(texture(2, 40));
    overlay.materials[0].texture = TextureRef::new(2, 0).unwrap();
    overlay.materials.push(Material {
        texture: base.material(2).texture,
        ..Material::unvaried()
    });
    overlay.visuals[0].faces = [0, 1, 0, 1, 0, 1];
    overlay.material_overrides = [
        TextureRef::new(2, 0).unwrap(),
        TextureRef::new(3, 1).unwrap(),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, texture)| MaterialOverride {
        material: index as u32 + 1,
        texture,
        animation: NO_ANIMATION,
        variation: None,
    })
    .collect();
    overlay.texture_source_sizes = vec![[8; 2], [16; 2], [8; 2]];
    overlay.texture_source_grids = vec![1, 0, 1];
    overlay.max_page_layers = crate::MAX_CARRIER_TEXTURE_LAYERS as u32;
    overlay.animation_frames = vec![
        TextureRef::new(2, 0).unwrap(),
        TextureRef::new(3, 1).unwrap(),
    ];
    overlay.animations.push(Animation {
        frame_start: 0,
        frame_count: 2,
        ticks_per_frame: 1,
        atlas_index: 0,
        atlas_tile_variant: 0,
        replicate: 1,
        flags: 0,
    });
    overlay.materials[0].animation = 0;
    let session = base.with_block_overlay(1, &overlay).unwrap();
    assert_eq!(session.animation_frames(), overlay.animation_frames);
    assert_eq!(session.material(3).animation, 0);
    for replacement in &overlay.material_overrides {
        assert_eq!(
            session.material(replacement.material).texture,
            replacement.texture
        );
    }
    let visual = session.resolve(NetworkIdMode::Sequential, 1);
    assert!(visual.is_known());
    assert_eq!(
        session
            .material(visual.face(BlockFace::West).material_id())
            .texture,
        TextureRef::new(2, 0).unwrap()
    );
    assert_eq!(
        session
            .material(visual.face(BlockFace::East).material_id())
            .texture,
        base.material(2).texture
    );
    for (reference, size, grid, color) in [
        (TextureRef::new(1, 575).unwrap(), 16, 0, 20),
        (TextureRef::new(2, 0).unwrap(), 8, 1, 30),
        (TextureRef::new(3, 1).unwrap(), 8, 1, 40),
    ] {
        assert_eq!(session.texture_source_size(reference), [size; 2]);
        assert_eq!(session.texture_source_grid(reference), grid);
        let page = session
            .terrain_texture_page(reference.page() as usize)
            .unwrap();
        let start = reference.layer() as usize * 16 * 16 * 4;
        assert_eq!(
            &page.mips[0].rgba8[start..start + 4],
            &[color, reference.layer() as u8, 0, 255]
        );
    }
    assert_eq!(session.texture_shards().len(), overlay.textures.len());
    overlay.base_texture_pages = 1;
    assert!(base.with_block_overlay(1, &overlay).is_err());
}

#[test]
fn multi_page_base_rejects_page_and_device_overflow_without_changing_base() {
    let base = base();
    let mut overlay = cube_overlay(texture(1, 30));
    overlay.base_texture_pages = 2;
    overlay.materials[0].texture = TextureRef::new(2, 0).unwrap();
    overlay
        .textures
        .resize(MAX_TEXTURE_PAGES - base.texture_pages.len(), texture(1, 30));
    assert!(base.with_block_overlay(1, &overlay).is_ok());
    overlay.textures.push(texture(1, 30));
    assert!(base.with_block_overlay(1, &overlay).is_err());
    overlay.textures.truncate(1);
    overlay.max_page_layers = crate::MAX_CARRIER_TEXTURE_LAYERS as u32;
    overlay.textures[0] = texture(overlay.max_page_layers + 1, 30);
    assert!(base.with_block_overlay(1, &overlay).is_err());
    overlay.textures[0] = texture(1, 30);
    overlay.max_page_layers -= 1;
    assert!(base.with_block_overlay(1, &overlay).is_err());
    assert_eq!(base.texture_pages.len(), 2);
    assert_eq!(base.material(2).texture, TextureRef::new(1, 575).unwrap());
}
