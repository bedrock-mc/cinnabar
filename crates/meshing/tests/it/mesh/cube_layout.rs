/// Overlay ids 1..=4: solid, alpha-tested, two-sided, and a positional solid material.
fn layout_fixture_assets() -> RuntimeAssets {
    let base = RuntimeAssets::diagnostic();
    let texture = TextureRef::new(1, 0).unwrap();
    let mut page = base.texture_array().clone();
    page.layers = 1;
    let visual = |material: u32| BlockVisual {
        faces: [material; 6],
        flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
        kind: VisualKind::Cube,
        support: assets::VisualSupport::VanillaFallback,
        contributor_role: assets::ContributorRole::Primary,
        model_template: NO_MODEL_TEMPLATE,
        animation: NO_ANIMATION,
        variant: 0,
    };
    let material = |flags: u32| Material {
        texture,
        flags,
        ..Material::unvaried()
    };
    let variant = Material {
        variation_weight: 0.5_f32.to_bits(),
        ..material(0)
    };
    let overlay = assets::BlockOverlay {
        visuals: vec![visual(0), visual(1), visual(2), visual(3)],
        light_properties: vec![assets::LightProperties::OPAQUE_DARK; 4],
        materials: vec![
            material(0),
            material(MATERIAL_FLAG_ALPHA_CUTOUT),
            material(MATERIAL_FLAG_ALPHA_CUTOUT | assets::MATERIAL_FLAG_TWO_SIDED),
            Material {
                variation_start: 4,
                variation_count: 2,
                ..material(0)
            },
            variant,
            variant,
        ],
        texture: Some(page),
        ..Default::default()
    };
    base.with_block_overlay(1, &overlay).unwrap()
}

/// Isolated blocks of every kind, so all six faces of each are exposed.
fn layout_fixture_sub_chunk() -> SubChunk {
    let mut placements = Vec::new();
    for x in (0..16_u8).step_by(2) {
        for y in (0..16_u8).step_by(2) {
            for z in (0..16_u8).step_by(2) {
                let kind = usize::from((x + y / 2 + z / 4) % 4);
                placements.push(([x, y, z], kind + 1));
            }
        }
    }
    sub_chunk(vec![packed_storage(3, &[AIR, 1, 2, 3, 4], &placements)])
}

#[test]
fn cube_layout_partitions_solid_quads_into_face_runs_ahead_of_two_sided_quads() {
    let assets = layout_fixture_assets();
    let mesh = mesh_sub_chunk(
        &classifier(),
        &assets,
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &layout_fixture_sub_chunk(),
    );
    let quads = mesh.cube_quads();
    let layout = mesh.cube_layout();
    let solid_len = layout.solid_len() as usize;
    assert_eq!(quads.len(), mesh.cube_lighting().len());
    assert!(solid_len > 0 && solid_len < quads.len());
    let solid =
        |quad: &PackedQuad| meshing::is_single_sided_opaque(assets.materials(), quad.material_id());
    assert!(quads[..solid_len].iter().all(solid));
    assert!(!quads[solid_len..].iter().any(solid));

    let mut covered = vec![0_u8; solid_len];
    for face in Face::ALL {
        let range = layout.solid_range(face);
        assert!(!range.is_empty(), "{face:?} has solid quads in the fixture");
        for index in range {
            covered[index as usize] += 1;
            assert_eq!(quads[index as usize].face(), face);
        }
    }
    assert!(covered.iter().all(|&count| count == 1));
    assert_eq!(layout.checked(quads, assets.materials()), layout);
}

#[test]
fn cube_layout_reorders_quads_without_changing_the_emitted_set() {
    let assets = layout_fixture_assets();
    let mesh = mesh_sub_chunk(
        &classifier(),
        &assets,
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &layout_fixture_sub_chunk(),
    );
    // Each quad keeps its own lighting record through the partition.
    let mut pairs = mesh
        .cube_quads()
        .iter()
        .zip(mesh.cube_lighting())
        .map(|(quad, light)| (quad.words(), light.samples()))
        .collect::<Vec<_>>();
    let isolated = 8 * 8 * 8 * 6;
    assert_eq!(pairs.len(), isolated);
    pairs.sort_unstable();
    pairs.dedup_by_key(|(quad, _)| *quad);
    assert_eq!(pairs.len(), isolated);
}

#[test]
fn cube_layout_falls_back_to_two_sided_when_materials_disagree() {
    let assets = layout_fixture_assets();
    let mesh = mesh_sub_chunk(
        &classifier(),
        &assets,
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &layout_fixture_sub_chunk(),
    );
    let mut materials = assets.materials().to_vec();
    for material in &mut materials {
        material.flags |= MATERIAL_FLAG_ALPHA_CUTOUT;
    }
    let checked = mesh.cube_layout().checked(mesh.cube_quads(), &materials);
    assert_eq!(checked, meshing::CubeQuadLayout::default());
    assert_eq!(checked.solid_len(), 0);
}
