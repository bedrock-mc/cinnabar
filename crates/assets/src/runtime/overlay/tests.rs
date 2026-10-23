use super::*;
use crate::{BlockFlags, ContributorRole, NetworkIdMode, TextureMip, VisualSupport};

fn page(size: u32) -> TextureArray {
    let mut mips = Vec::new();
    let mut side = size;
    while side > 0 {
        mips.push(TextureMip {
            size: side,
            rgba8: vec![255; (side * side * 4) as usize].into(),
        });
        side /= 2;
    }
    TextureArray {
        layers: 1,
        mips: mips.into(),
    }
}

#[test]
fn review_zero_sized_extra_mips_are_rejected() {
    let mut texture = page(16);
    let mut mips = texture.mips.to_vec();
    mips.push(TextureMip {
        size: 0,
        rgba8: Box::new([]),
    });
    texture.mips = mips.into();
    assert!(
        RuntimeAssets::diagnostic()
            .with_block_overlay(1, &cube_overlay(texture))
            .is_err()
    );
}

#[test]
fn review_full_face_occlusion_requires_drawable_geometry() {
    let mut overlay = cube_overlay(page(16));
    overlay.visuals[0].kind = VisualKind::Model;
    overlay.visuals[0].flags = BlockFlags::OCCLUDES_FULL_FACE;
    overlay.visuals[0].model_template = 0;
    overlay.model_templates = vec![ModelTemplate {
        quad_start: 0,
        quad_count: 0,
        flags: 0,
    }];
    assert!(
        RuntimeAssets::diagnostic()
            .with_block_overlay(1, &overlay)
            .is_err()
    );
}

/// Creates one cube whose faces use the first overlay texture.
pub(super) fn cube_overlay(texture: TextureArray) -> BlockOverlay {
    BlockOverlay {
        visuals: vec![BlockVisual {
            native_material: crate::native_material::NativeMaterialFacts::Unresolved,
            faces: [0; 6],
            flags: BlockFlags::CUBE_GEOMETRY,
            kind: VisualKind::Cube,
            support: VisualSupport::VanillaFallback,
            contributor_role: ContributorRole::Primary,
            model_template: NO_MODEL_TEMPLATE,
            animation: NO_ANIMATION,
            variant: 0,
            destroy: crate::compiled::block_destroy::BlockDestroyFacts::Unresolved,
        }],
        light_properties: vec![LightProperties::OPAQUE_DARK],
        materials: vec![Material {
            texture: TextureRef::new(1, 0).unwrap(),
            flags: 0,
            animation: NO_ANIMATION,
            ..crate::Material::unvaried()
        }],
        textures: vec![texture],
        ..BlockOverlay::default()
    }
}

/// Checks that both texture access paths borrow the same retained mip buffers.
fn assert_shared_shards<'a>(
    shards: impl IntoIterator<Item = &'a TextureArray>,
    pages: &[TexturePage],
) {
    for (shard, page) in shards.into_iter().zip(pages) {
        for (mip, page_mip) in shard.mips.iter().zip(&page.texture.mips) {
            assert!(std::ptr::eq(mip.rgba8.as_ptr(), page_mip.rgba8.as_ptr()));
        }
    }
}

#[test]
fn overlay_shards_share_the_runtime_page_allocations() {
    let base = RuntimeAssets::diagnostic();
    let mut overlay = cube_overlay(page(16));
    overlay.textures.push(page(32));
    let runtime = base.with_block_overlay(1, &overlay).unwrap();
    assert_eq!(runtime.texture_shards().len(), overlay.textures.len());
    assert_shared_shards(runtime.texture_shards(), &runtime.texture_pages()[1..]);
    assert_eq!(base.texture_shards().len(), 0);
}

#[test]
fn overlay_source_dimensions_are_bounded_by_their_own_shard() {
    let mut overlay = cube_overlay(page(16));
    overlay.textures.push(page(64));
    overlay.texture_source_sizes = vec![[16; 2], [64; 2]];
    let base = RuntimeAssets::diagnostic();
    let runtime = base.with_block_overlay(1, &overlay).unwrap();
    assert_eq!(
        runtime.texture_source_size(TextureRef::new(1, 0).unwrap()),
        [16; 2]
    );
    assert_eq!(
        runtime.texture_source_size(TextureRef::new(2, 0).unwrap()),
        [64; 2]
    );
    overlay.texture_source_sizes[0] = [64; 2];
    assert!(base.with_block_overlay(1, &overlay).is_err());
}

#[test]
fn overlay_source_grids_are_bounded_by_their_own_shard() {
    let mut overlay = cube_overlay(page(16));
    overlay.textures.push(page(64));
    overlay.texture_source_sizes = vec![[16; 2]; 2];
    overlay.texture_source_grids = vec![0, 2];
    let base = RuntimeAssets::diagnostic();
    assert!(base.with_block_overlay(1, &overlay).is_ok());
    overlay.texture_source_grids[0] = 2;
    assert!(base.with_block_overlay(1, &overlay).is_err());
}

// Overlay hashes resolve to the overlay's ids and never shadow a base hash.
#[test]
fn overlay_hashes_extend_the_hash_table() {
    let base = RuntimeAssets::diagnostic();
    let mut overlay = cube_overlay(page(16));
    overlay.hashes = vec![Some(0xdead_beef)];
    let session = base.with_block_overlay(1, &overlay).unwrap();
    assert_eq!(session.sequential_id_for_hash(0xdead_beef), Some(1));
    overlay.hashes = vec![Some(1), Some(2)];
    assert!(base.with_block_overlay(1, &overlay).is_err());
}

// A base material can be repointed at the overlay page; the diagnostic material cannot.
#[test]
fn material_overrides_repoint_base_materials() {
    let base = RuntimeAssets::diagnostic();
    let mut overlay = cube_overlay(page(16));
    overlay.material_overrides = vec![MaterialOverride {
        material: 0,
        texture: TextureRef::new(1, 0).unwrap(),
        animation: NO_ANIMATION,
        variation: Some((0, 0)),
    }];
    assert!(base.with_block_overlay(1, &overlay).is_err());
}

#[test]
fn raster_only_leaf_replacement_preserves_the_base_weighted_selector() {
    let mut base = RuntimeAssets::diagnostic();
    base.materials = vec![
        Material::unvaried(),
        Material {
            variation_start: 2,
            variation_count: 2,
            ..Material::unvaried()
        },
        Material {
            variation_weight: 0.25_f32.to_bits(),
            ..Material::unvaried()
        },
        Material {
            variation_weight: 0.75_f32.to_bits(),
            ..Material::unvaried()
        },
    ]
    .into();
    let mut overlay = cube_overlay(page(16));
    overlay.material_overrides.push(MaterialOverride {
        material: 3,
        texture: TextureRef::new(1, 0).unwrap(),
        animation: NO_ANIMATION,
        variation: None,
    });
    let runtime = base.with_block_overlay(1, &overlay).unwrap();
    assert_eq!(runtime.material(1), base.material(1));
    assert_eq!(
        runtime.material(3).variation_weight,
        base.material(3).variation_weight
    );
    assert_eq!(runtime.material(3).texture.page(), 1);
    assert_eq!(base.material(3).texture.page(), 0);
}

#[test]
fn weighted_override_ranges_are_checked_before_offsetting() {
    let mut base = RuntimeAssets::diagnostic();
    base.materials = vec![Material::unvaried(); 2].into();
    let mut overlay = cube_overlay(page(16));
    for variation in [Some((u32::MAX, 2)), Some((1, 0)), Some((1, 2))] {
        overlay.material_overrides = vec![MaterialOverride {
            material: 1,
            texture: TextureRef::new(1, 0).unwrap(),
            animation: NO_ANIMATION,
            variation,
        }];
        assert!(base.with_block_overlay(1, &overlay).is_err());
    }
}

#[test]
fn replacing_a_selector_uses_the_server_texture_without_changing_leaf_weights() {
    let mut base = RuntimeAssets::diagnostic();
    base.materials = vec![
        Material::unvaried(),
        Material {
            variation_start: 2,
            variation_count: 1,
            ..Material::unvaried()
        },
        Material {
            variation_weight: 1.0_f32.to_bits(),
            ..Material::unvaried()
        },
    ]
    .into();
    let (original, _, _) = crate::gui_item::material_tile(base.materials(), 1, |page| {
        base.texture_pages()
            .get(page as usize)
            .map(|page| &page.texture)
    })
    .unwrap();
    let mut overlay = cube_overlay(page(16));
    overlay.material_overrides = vec![MaterialOverride {
        material: 1,
        texture: TextureRef::new(1, 0).unwrap(),
        animation: NO_ANIMATION,
        variation: None,
    }];
    let session = base.with_block_overlay(1, &overlay).unwrap();
    assert_eq!(session.materials[1].variation_start, 2);
    assert_eq!(session.materials[1].variation_count, 1);
    assert_eq!(session.materials[1].texture.page(), 1);
    assert_eq!(session.materials[2].variation_weight, 1.0_f32.to_bits());
    let (replacement, side, _) = crate::gui_item::material_tile(session.materials(), 1, |page| {
        session
            .texture_pages()
            .get(page as usize)
            .map(|page| &page.texture)
    })
    .unwrap();
    assert_eq!(side, 16);
    assert!(
        replacement
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| *pixel == [255; 4])
    );
    assert_ne!(replacement, original);
    overlay.material_overrides[0].variation = Some((0, 0));
    let cleared = base.with_block_overlay(1, &overlay).unwrap();
    assert_eq!(cleared.materials[1].variation_count, 0);
    assert_eq!(cleared.materials[2].variation_weight, 1.0_f32.to_bits());
}

// A 32px page is accepted; malformed mips or dangling ids are refused whole.
#[test]
fn overlay_page_and_references_are_validated() {
    let base = RuntimeAssets::diagnostic();
    let session = base.with_block_overlay(1, &cube_overlay(page(32))).unwrap();
    let block = session.resolve(NetworkIdMode::Sequential, 1);
    assert_eq!(
        session
            .material(block.face(crate::BlockFace::Up).material_id())
            .texture
            .page(),
        1
    );

    let mut truncated = page(32);
    truncated.mips = truncated.mips[..3].into();
    assert!(
        base.with_block_overlay(1, &cube_overlay(truncated))
            .is_err()
    );
    let mut dangling = cube_overlay(page(16));
    dangling.visuals[0].faces[2] = 5;
    assert!(base.with_block_overlay(1, &dangling).is_err());
}

#[test]
fn source_pixel_dimensions_survive_overlay_admission() {
    let base = RuntimeAssets::diagnostic();
    let mut overlay = cube_overlay(page(64));
    overlay.texture_source_sizes = vec![[16, 8]];
    let session = base.with_block_overlay(1, &overlay).unwrap();
    assert_eq!(
        session.texture_source_size(TextureRef::new(1, 0).unwrap()),
        [16, 8]
    );
    assert_eq!(session.texture_source_size(TextureRef::DIAGNOSTIC), [16; 2]);
    overlay.texture_source_grids = vec![1];
    let gridded = base.with_block_overlay(1, &overlay).unwrap();
    assert_eq!(
        gridded.texture_source_grid(TextureRef::new(1, 0).unwrap()),
        1
    );
    assert_eq!(gridded.texture_source_grid(TextureRef::DIAGNOSTIC), 0);
    for grids in [vec![1, 1], vec![3], vec![31], vec![32]] {
        overlay.texture_source_grids = grids;
        assert!(base.with_block_overlay(1, &overlay).is_err());
    }
    overlay.texture_source_grids.clear();
    for sizes in [
        vec![[0, 8]],
        vec![[128, 8]],
        vec![[15, 8]],
        vec![[16, 8]; 2],
    ] {
        overlay.texture_source_sizes = sizes;
        assert!(base.with_block_overlay(1, &overlay).is_err());
    }
}

#[test]
fn expanded_layers_retain_their_source_mips_for_terrain_upload() {
    let mut texture = page(64);
    for (index, pixel) in texture.mips[0]
        .rgba8
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .enumerate()
    {
        pixel.copy_from_slice(if index % 8 < 4 {
            &[255, 0, 0, 255]
        } else {
            &[0, 0, 255, 255]
        });
    }
    for mip in &mut texture.mips[1..] {
        for pixel in mip.rgba8.as_chunks_mut::<4>().0 {
            pixel.copy_from_slice(&[127, 0, 127, 255]);
        }
    }
    let mut overlay = cube_overlay(texture);
    overlay.texture_source_sizes = vec![[16; 2]];
    let session = RuntimeAssets::diagnostic()
        .with_block_overlay(1, &overlay)
        .unwrap();
    let uploaded = session.terrain_texture_page(1).unwrap();
    assert_eq!(&uploaded.mips[1].rgba8[..4], &[127, 0, 127, 255]);
    assert!(session.terrain_texture_page(2).is_err());
}

// Fallback resolves static base textures by terrain key and skips
// diagnostic, animated, and unknown entries.
#[test]
fn vanilla_fallback_resolves_static_base_textures() {
    let keys = MaterialKeys::from_entries([(1, "oak_planks"), (2, "water")]);
    let materials = [
        Material::unvaried(),
        Material {
            texture: TextureRef::new(0, 0).unwrap(),
            ..Material::unvaried()
        },
        Material {
            texture: TextureRef::new(0, 0).unwrap(),
            animation: 0,
            ..Material::unvaried()
        },
    ];
    let fallback = VanillaFallback::new(&keys, &materials);
    let reference = fallback.texture("oak_planks", None).expect("base oak");
    assert_eq!((reference.page(), reference.layer()), (0, 0));
    assert!(fallback.texture("water", None).is_none());
    assert!(fallback.texture("missing", None).is_none());
    let diagnostic = MaterialKeys::from_entries([(0, "oak_planks")]);
    assert!(
        VanillaFallback::new(&diagnostic, &materials)
            .texture("oak_planks", None)
            .is_none()
    );
}

#[test]
fn vanilla_fallback_uses_catalog_variant_zero_instead_of_material_order() {
    let materials = [
        Material::unvaried(),
        Material {
            texture: TextureRef::new(0, 1).unwrap(),
            ..Material::unvaried()
        },
        Material {
            texture: TextureRef::new(0, 2).unwrap(),
            ..Material::unvaried()
        },
    ];
    for first_destination in [(1, 0), (0, 1)] {
        let keys = MaterialKeys::from_entries([(2, "farmland"), (1, "farmland")])
            .with_aliases([("farmland", "textures/blocks/farmland_wet")])
            .with_flipbook_selectors([(1, first_destination), (2, (0, 0))]);
        let keys = MaterialKeys::from_json(&keys.to_json(materials.len() as u32), materials.len())
            .unwrap();
        let fallback = VanillaFallback::new(&keys, &materials);
        assert_eq!(
            fallback.texture("farmland", None),
            Some(materials[2].texture)
        );
    }
}

#[test]
fn vanilla_fallback_does_not_substitute_another_static_variant() {
    let keys = MaterialKeys::from_entries([(1, "farmland"), (2, "farmland")])
        .with_flipbook_selectors([(1, (1, 0)), (2, (0, 0))]);
    for variant_zero in [
        Material {
            animation: 0,
            ..Material::unvaried()
        },
        Material {
            variation_count: 1,
            ..Material::unvaried()
        },
    ] {
        let materials = [Material::unvaried(), Material::unvaried(), variant_zero];
        assert!(
            VanillaFallback::new(&keys, &materials)
                .texture("farmland", None)
                .is_none()
        );
    }
    let keys = MaterialKeys::from_entries([(1, "farmland")]).with_flipbook_selectors([(1, (1, 0))]);
    assert!(
        VanillaFallback::new(&keys, &[Material::unvaried(); 2])
            .texture("farmland", None)
            .is_none()
    );
}

// Overlay materials may borrow a base page layer; out-of-range layers,
// borrowed animations, and page-0 overrides stay rejected.
#[test]
fn overlay_materials_may_borrow_the_base_page() {
    let base = RuntimeAssets::diagnostic();
    let mut overlay = cube_overlay(page(16));
    overlay.materials[0].texture = TextureRef::new(0, 0).unwrap();
    let session = base.with_block_overlay(1, &overlay).unwrap();
    assert_eq!(session.materials[1].texture.page(), 0);

    let mut out_of_range = cube_overlay(page(16));
    out_of_range.materials[0].texture = TextureRef::new(0, 1).unwrap();
    assert!(base.with_block_overlay(1, &out_of_range).is_err());

    let mut animated = cube_overlay(page(16));
    animated.materials[0].texture = TextureRef::new(0, 0).unwrap();
    animated.materials[0].animation = 0;
    assert!(base.with_block_overlay(1, &animated).is_err());
}

// Overrides repoint base materials at overlay art; the base page is not
// an overlay source.
#[test]
fn overlay_overrides_reject_the_base_page() {
    let mut base = RuntimeAssets::diagnostic();
    base.materials = vec![Material::unvaried(), Material::unvaried()].into();
    let mut overlay = cube_overlay(page(16));
    overlay.material_overrides = vec![MaterialOverride {
        material: 1,
        texture: TextureRef::new(0, 0).unwrap(),
        animation: NO_ANIMATION,
        variation: None,
    }];
    assert!(base.with_block_overlay(1, &overlay).is_err());
}
