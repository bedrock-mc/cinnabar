const TWIN_AIR: u32 = 0;
const TWIN_A: u32 = 1;
const TWIN_B: u32 = 2;
const TWIN_OTHER: u32 = 3;
const TWIN_LIQUID: u32 = 4;
const TWIN_UNREGISTERED: u32 = 99;

/// Air, two distinct ids with identical cube visuals and light, a visibly different cube, and
/// a liquid.
static TWIN_CUBE_ASSETS: std::sync::LazyLock<RuntimeAssets> = std::sync::LazyLock::new(|| {
    let visual = |faces, flags, kind, contributor_role| BlockVisual {
        native_material: assets::native_material::NativeMaterialFacts::Unresolved,
        support: assets::VisualSupport::Exact,
        faces,
        flags,
        kind,
        contributor_role,
        model_template: NO_MODEL_TEMPLATE,
        animation: NO_ANIMATION,
        variant: 0,
        destroy: assets::compiled::block_destroy::BlockDestroyFacts::Unresolved,
    };
    let cube = |material| {
        visual(
            [material; 6],
            BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
            VisualKind::Cube,
            assets::ContributorRole::Primary,
        )
    };
    let visuals = vec![
        visual(
            [DIAGNOSTIC_MATERIAL; 6],
            BlockFlags::AIR,
            VisualKind::Invisible,
            assets::ContributorRole::Air,
        ),
        cube(1),
        cube(1),
        cube(2),
        visual(
            [2; 6],
            BlockFlags::empty(),
            VisualKind::Liquid,
            assets::ContributorRole::LiquidAdditional,
        ),
    ];
    let material = Material {
        texture: TextureRef::DIAGNOSTIC,
        flags: 0,
        animation: NO_ANIMATION,
        ..assets::Material::unvaried()
    };
    let compiled = CompiledAssets {
        light_properties: vec![assets::LightProperties::default(); visuals.len()]
            .into_boxed_slice(),
        visuals: visuals.into_boxed_slice(),
        hashed: Box::new([]),
        materials: vec![material; 3].into_boxed_slice(),
        model_templates: Box::new([]),
        model_quads: Box::new([]),
        animations: Box::new([]),
        animation_frames: Box::new([]),
        texture_pages: vec![TexturePage::new(TextureArray {
            layers: 1,
            mips: [16_u32, 8, 4, 2, 1]
                .map(|size| TextureMip {
                    size,
                    rgba8: vec![0xff; size as usize * size as usize * 4].into_boxed_slice(),
                })
                .into(),
        })]
        .into_boxed_slice(),
        biomes: CompiledBiomeAssets::diagnostic(),
        provenance: assets::BlobProvenance {
            source_manifest_sha256: [0xA5; 32],
            block_registry_sha256: [0x5A; 32],
            light_registry_sha256: [0x33; 32],
            biome_registry_sha256: [0x3C; 32],
        },
    };
    RuntimeAssets::decode(&encode_blob(&compiled).unwrap()).unwrap()
});

fn twin_cube_assets() -> &'static RuntimeAssets {
    &TWIN_CUBE_ASSETS
}

/// The changed boundary cell sits beside an unchanged twin and a different cube.
fn twin_cube_section(changed: u32) -> SubChunk {
    let placements = [([15, 4, 4], 1), ([14, 4, 4], 2), ([3, 3, 3], 3)];
    sub_chunk(vec![packed_storage(
        2,
        &[TWIN_AIR, changed, TWIN_A, TWIN_OTHER],
        &placements,
    )])
}

fn twin_preserves(previous: &SubChunk, replacement: &SubChunk, hint: &[[u8; 3]]) -> bool {
    meshing::block_changes_preserve_meshes(
        BlockClassifier::new(TWIN_AIR),
        twin_cube_assets(),
        NetworkIdMode::Sequential,
        previous,
        replacement,
        hint.iter().copied(),
        &[],
    )
}

#[test]
fn twin_visual_swap_is_proven_and_leaves_own_and_neighbour_meshes_unchanged() {
    let previous = twin_cube_section(TWIN_A);
    let replacement = twin_cube_section(TWIN_B);
    assert!(twin_preserves(&previous, &replacement, &[[15, 4, 4]]));
    // Unhinted differences are found by the whole-section comparison.
    assert!(twin_preserves(&previous, &replacement, &[]));

    let classifier = BlockClassifier::new(TWIN_AIR);
    let assets = twin_cube_assets();
    let mode = NetworkIdMode::Sequential;
    let mesh = |neighbours: &Neighbourhood<'_>, section: &SubChunk| {
        mesh_sub_chunk(&classifier, assets, mode, neighbours, section)
    };
    assert_eq!(
        mesh(&Neighbourhood::empty(), &previous),
        mesh(&Neighbourhood::empty(), &replacement)
    );
    // The east neighbour samples the changed boundary cell.
    let east = sub_chunk(vec![packed_storage(
        1,
        &[TWIN_AIR, TWIN_OTHER],
        &[([0, 4, 4], 1), ([0, 5, 4], 1)],
    )]);
    assert_eq!(
        mesh(&neighbourhood_for(Face::NegativeX, &previous), &east),
        mesh(&neighbourhood_for(Face::NegativeX, &replacement), &east)
    );
}

#[test]
fn visible_air_liquid_unregistered_or_unhinted_changes_are_not_proven() {
    let previous = twin_cube_section(TWIN_A);
    for changed in [TWIN_OTHER, TWIN_AIR, TWIN_LIQUID, TWIN_UNREGISTERED] {
        let replacement = twin_cube_section(changed);
        assert!(
            !twin_preserves(&previous, &replacement, &[[15, 4, 4]]),
            "{changed} is not mesh-neutral"
        );
        assert!(
            !twin_preserves(&previous, &replacement, &[]),
            "unhinted {changed} is still found"
        );
    }
    // A twin swap hinted alongside an unhinted visible change is rejected.
    let mixed = sub_chunk(vec![packed_storage(
        2,
        &[TWIN_AIR, TWIN_B, TWIN_A, TWIN_A],
        &[([15, 4, 4], 1), ([14, 4, 4], 2), ([3, 3, 3], 3)],
    )]);
    assert!(!twin_preserves(&previous, &mixed, &[[15, 4, 4]]));
    // The proof itself never counts a registry miss.
    let misses = twin_cube_assets().missing_count();
    assert!(!twin_preserves(
        &previous,
        &twin_cube_section(TWIN_UNREGISTERED),
        &[]
    ));
    assert_eq!(twin_cube_assets().missing_count(), misses);
}

#[test]
fn a_swap_inside_a_failed_closed_cell_is_not_proven() {
    // Two primaries in one cell merge into a diagnostic that carries a raw value.
    let layered = |changed| {
        sub_chunk(vec![
            packed_storage(1, &[TWIN_AIR, changed], &[([8, 8, 8], 1)]),
            packed_storage(1, &[TWIN_AIR, TWIN_OTHER], &[([8, 8, 8], 1)]),
        ])
    };
    assert!(!twin_preserves(
        &layered(TWIN_A),
        &layered(TWIN_B),
        &[[8, 8, 8]]
    ));
    // A changed layer count is never proven.
    assert!(!twin_preserves(
        &twin_cube_section(TWIN_A),
        &layered(TWIN_B),
        &[]
    ));
}

#[test]
fn water_filled_alias_changes_are_not_mesh_neutral() {
    let previous = twin_cube_section(TWIN_A);
    let replacement = twin_cube_section(TWIN_B);
    for hint in [vec![], vec![[15, 4, 4]]] {
        for filled in [&[TWIN_A][..], &[TWIN_B][..], &[TWIN_A, TWIN_B][..]] {
            assert_eq!(
                meshing::block_changes_preserve_meshes(
                    BlockClassifier::new(TWIN_AIR),
                    twin_cube_assets(),
                    NetworkIdMode::Sequential,
                    &previous,
                    &replacement,
                    hint.iter().copied(),
                    filled,
                ),
                filled.len() == 2,
                "hint {hint:?}, filled {filled:?}",
            );
        }
    }
}
