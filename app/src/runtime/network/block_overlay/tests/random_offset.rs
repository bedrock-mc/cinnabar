use super::*;

#[test]
fn random_offset_state_authority_drives_mesh_and_overlay_geometry_in_both_id_spaces() {
    let base = block_transform::random_offset::BAMBOO;
    let zero = block_transform::random_offset::RandomOffsetComponent::default();
    let mut custom = generator();
    custom.state_physics = [base, zero, base, base]
        .map(|component| {
            let mut physics = custom.base_physics();
            physics.random_offset = Some(component);
            physics
        })
        .into();
    let blocks = CustomBlocks {
        blocks: Arc::from([custom]),
        ..Default::default()
    };
    for hashed in [false, true] {
        let compiled = compile_block_overlay(&view(), &blocks, hashed, None).unwrap();
        let assets = RuntimeAssets::diagnostic()
            .with_block_overlay(1, &compiled.overlay)
            .unwrap();
        for (state, component) in [base, zero, base, base].into_iter().enumerate() {
            let id = if hashed {
                blocks.blocks[0].hashed_states()[state].hash
            } else {
                1 + state as u32
            };
            let mode = if hashed {
                NetworkIdMode::Hashed
            } else {
                NetworkIdMode::Sequential
            };
            let visual = assets.resolve(mode, id);
            let template = visual.model_template().unwrap();
            assert_eq!(assets.model_random_offset(template), Some(component));
            let position = [1, 8, 0];
            let key = world::SubChunkKey::new(0, 0, 0, 0);
            let mut store = world::ChunkStore::new();
            store.mark_sub_chunk_loaded(key).unwrap();
            store
                .update_block(key, world::BlockUpdate::new(1, 8, 0, 0, id), 0)
                .unwrap();
            let chunk = store.sub_chunk(key).unwrap();
            let mesh = meshing::mesh_sub_chunk(
                &meshing::BlockClassifier::new(0),
                &assets,
                mode,
                &meshing::Neighbourhood::empty(),
                &chunk,
            );
            let reference = mesh.model_refs()[0].words();
            assert_ne!(reference[0] & meshing::MODEL_REF_FLAG_RANDOM_OFFSET, 0);
            let index = reference[2] as usize;
            let offset = meshing::PackedQuadLighting::offset_from_prefix([
                mesh.model_lighting()[index - 2],
                mesh.model_lighting()[index - 1],
            ]);
            assert_eq!(offset, component.offset(position));
            let render::CrackShape::Quads(shape) =
                render::crack_shape_from_template(&assets, template, visual.variant(), position)
                    .unwrap()
            else {
                panic!("model surface");
            };
            let first = assets.model_quads()
                [assets.model_templates()[template as usize].quad_start as usize]
                .positions[0];
            assert_eq!(
                shape[0].corners[0],
                std::array::from_fn(|axis| f32::from(first[axis]) / 256.0 + offset[axis])
            );
        }
    }
}

#[test]
fn terrain_override_mips_average_unassociated_leaf_alpha_and_colours() {
    let view = view();
    let mut builder = super::super::Builder {
        catalog: super::super::textures::TextureCatalog::new(&view, None),
        geometries: Default::default(),
        overlay: Default::default(),
        sources: Vec::new(),
        source_bytes: 0,
        textures: Default::default(),
        materials: Default::default(),
        visuals: Default::default(),
        gaps: Default::default(),
    };
    let mut pixels = Vec::new();
    for y in 0..16 {
        for x in 0..16 {
            pixels.extend(if x % 2 == 0 && y % 2 == 0 {
                [200, 100, 40, 255]
            } else {
                [0, 0, 0, 0]
            });
        }
    }
    builder
        .sources
        .push(super::super::Source::Image(super::super::DecodedTexture {
            width: 16,
            height: 16,
            rgba8: pixels.into_boxed_slice(),
        }));
    let compiled = builder.finish().unwrap();
    let page = compiled.overlay.texture.unwrap();
    assert_eq!(&page.mips[1].rgba8[..4], &[50, 25, 10, 63]);
    assert_eq!(&page.mips[3].rgba8[..4], &[50, 25, 10, 63]);
}

#[test]
fn random_offset_fallback_cubes_keep_faces_exposed_by_their_offset() {
    let mut custom = generator();
    let base = Arc::make_mut(&mut custom.visual);
    base.base.geometry = None;
    let mut offset = block_transform::random_offset::RandomOffsetComponent::default();
    offset.axes[0].range = [0.25; 2];
    base.base.random_offset = Some(offset);
    let solid = block(
        "test:solid",
        1,
        CustomBlockVisuals {
            base: CustomVisualComponents {
                materials: materials("lucky"),
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let blocks = CustomBlocks {
        blocks: Arc::from([custom, solid]),
        ..Default::default()
    };
    let compiled = compile_block_overlay(&view(), &blocks, false, None).unwrap();
    let assets = RuntimeAssets::diagnostic()
        .with_block_overlay(1, &compiled.overlay)
        .unwrap();
    let key = world::SubChunkKey::new(0, 0, 0, 0);
    let mut store = world::ChunkStore::new();
    store.mark_sub_chunk_loaded(key).unwrap();
    for x in [0, 1] {
        store
            .update_block(
                key,
                world::BlockUpdate::new(x, 8, 0, 0, if x == 0 { 5 } else { 1 }),
                0,
            )
            .unwrap();
    }
    let mesh = meshing::mesh_sub_chunk(
        &meshing::BlockClassifier::new(0),
        &assets,
        NetworkIdMode::Sequential,
        &meshing::Neighbourhood::empty(),
        &store.sub_chunk(key).unwrap(),
    );
    assert_eq!(
        mesh.model_draw_refs().len(),
        6,
        "the shifted west face is exposed beside the undisplaced cube"
    );
}

#[test]
fn random_offset_authored_geometry_keeps_faces_exposed_by_its_offset() {
    let mut custom = generator();
    let base = Arc::make_mut(&mut custom.visual);
    base.base.geometry = Some("geometry.gen".into());
    let mut offset = block_transform::random_offset::RandomOffsetComponent::default();
    offset.axes[0].range = [0.25; 2];
    base.base.random_offset = Some(offset);
    let solid = block(
        "test:solid",
        1,
        CustomBlockVisuals {
            base: CustomVisualComponents {
                materials: materials("lucky"),
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let blocks = CustomBlocks {
        blocks: Arc::from([custom, solid]),
        ..Default::default()
    };
    let compiled = compile_block_overlay(
        &view_with_geometry(
            br#"{"format_version":"1.12.0","minecraft:geometry":[{
        "description":{"identifier":"geometry.gen","texture_width":16,"texture_height":16},
        "bones":[{"name":"block","cubes":[{"origin":[-8,0,-8],"size":[16,16,16],"uv":[0,0]}]}]
    }]}"#,
        ),
        &blocks,
        false,
        None,
    )
    .unwrap();
    let assets = RuntimeAssets::diagnostic()
        .with_block_overlay(1, &compiled.overlay)
        .unwrap();
    let key = world::SubChunkKey::new(0, 0, 0, 0);
    let mut store = world::ChunkStore::new();
    store.mark_sub_chunk_loaded(key).unwrap();
    for x in [0, 1] {
        store
            .update_block(
                key,
                world::BlockUpdate::new(x, 8, 0, 0, if x == 0 { 5 } else { 1 }),
                0,
            )
            .unwrap();
    }
    let mesh = meshing::mesh_sub_chunk(
        &meshing::BlockClassifier::new(0),
        &assets,
        NetworkIdMode::Sequential,
        &meshing::Neighbourhood::empty(),
        &store.sub_chunk(key).unwrap(),
    );
    assert_eq!(
        mesh.model_draw_refs().len(),
        6,
        "the shifted west face is exposed beside the undisplaced cube"
    );
}

/// Builds an overlay from decoded images without catalog or geometry inputs.
fn overlay_images(
    images: impl IntoIterator<Item = super::super::DecodedTexture>,
) -> assets::BlockOverlay {
    let view = view();
    super::super::Builder {
        catalog: super::super::textures::TextureCatalog::new(&view, None),
        geometries: Default::default(),
        overlay: Default::default(),
        sources: images
            .into_iter()
            .map(super::super::Source::Image)
            .collect(),
        source_bytes: 0,
        textures: Default::default(),
        materials: Default::default(),
        visuals: Default::default(),
        gaps: Default::default(),
    }
    .finish()
    .unwrap()
    .overlay
}

#[test]
fn terrain_override_small_images_keep_their_source_mips_beside_hd_images() {
    let small = super::super::DecodedTexture {
        width: 16,
        height: 16,
        rgba8: (0..256)
            .flat_map(|i| {
                if i % 2 == 0 {
                    [255, 0, 0, 255]
                } else {
                    [0, 0, 255, 255]
                }
            })
            .collect(),
    };
    let large = super::super::DecodedTexture {
        width: 64,
        height: 64,
        rgba8: vec![255; 64 * 64 * 4].into(),
    };
    let overlay = overlay_images([small, large]);
    let page = overlay.texture.unwrap();
    assert_eq!(page.mips[0].size, 64);
    assert_eq!(&page.mips[1].rgba8[..4], &[127, 0, 127, 255]);
    assert_eq!(&page.mips[3].rgba8[..4], &[127, 0, 127, 255]);
}

#[test]
fn terrain_override_static_vertical_strip_admits_its_first_square() {
    let strip = super::super::DecodedTexture {
        width: 16,
        height: 32,
        rgba8: (0..512)
            .flat_map(|i| {
                if i < 256 {
                    [255, 0, 0, 255]
                } else {
                    [0, 0, 255, 255]
                }
            })
            .collect(),
    };
    let overlay = overlay_images([strip]);
    let page = overlay.texture.unwrap();
    assert!(
        page.mips[0]
            .rgba8
            .chunks_exact(4)
            .all(|pixel| pixel == [255, 0, 0, 255])
    );
}
