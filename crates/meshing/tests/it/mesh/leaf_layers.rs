fn leaf_neighbour(origin: [u8; 3], face: Face) -> [u8; 3] {
    let offset = match face {
        Face::NegativeX => [-1, 0, 0],
        Face::PositiveX => [1, 0, 0],
        Face::NegativeY => [0, -1, 0],
        Face::PositiveY => [0, 1, 0],
        Face::NegativeZ => [0, 0, -1],
        Face::PositiveZ => [0, 0, 1],
    };
    std::array::from_fn(|axis| (i32::from(origin[axis]) + offset[axis]) as u8)
}

fn leaf_span_covers_cell(
    origin: [u8; 3],
    face: Face,
    width: u8,
    height: u8,
    cell: [u8; 3],
) -> bool {
    // Packed cube spans use the same (slice, u, v) axes as greedy_slice.
    // A cell's face can belong to a span whose origin is a neighbouring cell.
    let [slice, u, v] = match face {
        Face::NegativeX | Face::PositiveX => [0, 2, 1],
        Face::NegativeY | Face::PositiveY => [1, 0, 2],
        Face::NegativeZ | Face::PositiveZ => [2, 0, 1],
    };
    cell[slice] == origin[slice]
        && (origin[u]..origin[u] + width).contains(&cell[u])
        && (origin[v]..origin[v] + height).contains(&cell[v])
}

fn assert_leaf_layer(mesh: &ChunkMesh, assets: &RuntimeAssets, origin: [u8; 3], deep: bool) {
    let quads = mesh
        .cube_quads()
        .iter()
        .filter(|quad| {
            leaf_span_covers_cell(
                quad.origin(),
                quad.face(),
                quad.width(),
                quad.height(),
                origin,
            )
        })
        .collect::<Vec<_>>();
    assert!(
        !quads.is_empty(),
        "leaf {origin:?} must have a visible surface"
    );
    for quad in quads {
        let material = assets.materials()[quad.material_id() as usize];
        assert_eq!(
            material.flags & assets::MATERIAL_FLAG_ALPHA_CUTOUT != 0,
            !deep
        );
        assert_eq!(material.flags & assets::MATERIAL_FLAG_TWO_SIDED != 0, !deep);
        assert_eq!(material.texture, assets.materials()[1].texture);
    }
}

#[test]
fn leaf_layer_assertion_accounts_for_greedy_spans_on_every_face() {
    let origin = [6, 7, 8];
    for face in Face::ALL {
        let (normal, width, height) = match face {
            Face::NegativeX | Face::PositiveX => (0, 2, 1),
            Face::NegativeY | Face::PositiveY => (1, 0, 2),
            Face::NegativeZ | Face::PositiveZ => (2, 0, 1),
        };
        let mut covered = origin;
        covered[width] += 1;
        covered[height] += 2;
        assert!(
            leaf_span_covers_cell(origin, face, 2, 3, covered),
            "{face:?}"
        );
        for (axis, outside) in [(normal, 1), (width, 2), (height, 3)] {
            let mut uncovered = origin;
            uncovered[axis] += outside;
            assert!(
                !leaf_span_covers_cell(origin, face, 2, 3, uncovered),
                "{face:?}"
            );
        }
    }
}

#[test]
fn leaf_shared_plane_keeps_exact_native_down_south_east_half() {
    let assets = seasonal_leaf_fixture();
    let origin = [8, 8, 8];
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let classifier = BlockClassifier::new(if mode == NetworkIdMode::Hashed {
            0x10000
        } else {
            0
        });
        for (face, opposite) in [
            (Face::PositiveX, Face::NegativeX),
            (Face::PositiveY, Face::NegativeY),
            (Face::PositiveZ, Face::NegativeZ),
        ] {
            let neighbour = leaf_neighbour(origin, face);
            let chunk = seasonal_leaf_chunk(mode, &[(origin, 1), (neighbour, 1)]);
            let mesh = mesh_sub_chunk_in_neighbourhood(
                &classifier,
                &assets,
                mode,
                &MeshNeighbourhood::new(&chunk),
            );
            let first = mesh
                .cube_quads()
                .iter()
                .any(|quad| quad.origin() == origin && quad.face() == face);
            let second = mesh
                .cube_quads()
                .iter()
                .any(|quad| quad.origin() == neighbour && quad.face() == opposite);
            assert_eq!(first, face != Face::PositiveY, "{face:?}");
            assert_eq!(second, !first, "exactly one shared plane: {face:?}");
        }
    }
}

#[test]
fn deep_leaves_are_opaque_with_unchanged_fancy_texture_and_keep_faces_against_non_deep() {
    let assets = seasonal_leaf_fixture();
    let origin = [8, 8, 8];
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let classifier = BlockClassifier::new(if mode == NetworkIdMode::Hashed {
            0x10000
        } else {
            0
        });
        let mut placements = vec![(origin, 1)];
        placements.extend(Face::ALL.map(|face| (leaf_neighbour(origin, face), 1)));
        let chunk = seasonal_leaf_chunk(mode, &placements);
        let mesh = mesh_sub_chunk_in_neighbourhood(
            &classifier,
            &assets,
            mode,
            &MeshNeighbourhood::new(&chunk),
        );
        assert_leaf_layer(&mesh, &assets, origin, true);
        assert_eq!(
            mesh.cube_quads()
                .iter()
                .filter(|quad| quad.origin() == origin)
                .count(),
            Face::ALL.len()
        );
        for face in Face::ALL {
            let neighbour = leaf_neighbour(origin, face);
            assert_leaf_layer(&mesh, &assets, neighbour, false);
        }
    }
}

#[test]
fn deep_leaves_above_exception_is_top_snow_only_and_primary_layer_only() {
    let assets = seasonal_leaf_fixture();
    let origin = [8, 8, 8];
    let classifier = BlockClassifier::new(0);
    for (face, neighbour_id, deep) in [
        (Face::PositiveY, 4, true), // All native TopSnow heights carry property0x8.
        (Face::NegativeY, 4, false),
        (Face::PositiveX, 4, false),
        (Face::PositiveY, 8, false), // Diagnostic cannot prove solid rendering.
        (Face::PositiveY, 2, true),
    ] {
        let mut placements = vec![(origin, 1)];
        placements.extend(Face::ALL.map(|test_face| {
            (
                leaf_neighbour(origin, test_face),
                if face == test_face { neighbour_id } else { 1 },
            )
        }));
        let chunk = seasonal_leaf_chunk(NetworkIdMode::Sequential, &placements);
        let mesh = mesh_sub_chunk_in_neighbourhood(
            &classifier,
            &assets,
            NetworkIdMode::Sequential,
            &MeshNeighbourhood::new(&chunk),
        );
        assert_leaf_layer(&mesh, &assets, origin, deep);
    }
    let mut primary = vec![(origin, 1)];
    primary.extend(
        Face::ALL
            .into_iter()
            .filter(|&face| face != Face::PositiveX)
            .map(|face| (leaf_neighbour(origin, face), 1)),
    );
    let extra = [(leaf_neighbour(origin, Face::PositiveX), 1)];
    let chunk = seasonal_leaf_layered_chunk(NetworkIdMode::Sequential, &[&primary, &extra]);
    let mesh = mesh_sub_chunk_in_neighbourhood(
        &classifier,
        &assets,
        NetworkIdMode::Sequential,
        &MeshNeighbourhood::new(&chunk),
    );
    assert_leaf_layer(&mesh, &assets, origin, false);
}

#[test]
fn seasons_agnostic_leaves_use_native_depth_without_seasonal_colour() {
    let assets = seasonal_leaf_fixture();
    let origin = [8, 8, 8];
    let mut placements = vec![(origin, 1)];
    placements.extend(Face::ALL.map(|face| (leaf_neighbour(origin, face), 1)));
    // Dependency classification conservatively reads every palette entry,
    // including unused entries. Keep this palette strictly seasons-agnostic.
    let mut data = vec![9, 1, 0];
    data.extend(packed_storage(1, &[0, 9], &placements));
    let chunk = SubChunk::decode(&data, &RawBlockIds { air: 0 });
    let classifier = BlockClassifier::new(0);
    let mesh = mesh_sub_chunk_in_neighbourhood(
        &classifier,
        &assets,
        NetworkIdMode::Sequential,
        &MeshNeighbourhood::new(&chunk),
    );
    assert_leaf_layer(&mesh, &assets, origin, true);
    assert_eq!(
        mesh.cube_quads()
            .iter()
            .filter(|quad| quad.origin() == origin)
            .count(),
        Face::ALL.len()
    );
    for quad in mesh.cube_quads() {
        assert_eq!(
            assets.materials()[quad.material_id() as usize].flags
                & assets::MATERIAL_FLAG_SEASONAL_FOLIAGE,
            0
        );
    }
    assert!(
        !meshing::mesh_dependency_mask(&classifier, &assets, NetworkIdMode::Sequential, &chunk)
            .seasonal_foliage
    );
}

#[test]
fn deep_leaf_predicate_and_shared_plane_cross_every_primary_subchunk_boundary() {
    let assets = seasonal_leaf_fixture();
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let classifier = BlockClassifier::new(if mode == NetworkIdMode::Hashed {
            0x10000
        } else {
            0
        });
        for face in Face::ALL {
            let axis = match face {
                Face::NegativeX | Face::PositiveX => 0,
                Face::NegativeY | Face::PositiveY => 1,
                _ => 2,
            };
            let mut origin = [8, 8, 8];
            let negative = matches!(face, Face::NegativeX | Face::NegativeY | Face::NegativeZ);
            origin[axis] = if negative { 0 } else { 15 };
            let mut placements = vec![(origin, 1)];
            placements.extend(
                Face::ALL
                    .into_iter()
                    .filter(|&test| test != face)
                    .map(|test| (leaf_neighbour(origin, test), 1)),
            );
            let center = seasonal_leaf_chunk(mode, &placements);
            let mut adjacent = origin;
            adjacent[axis] = if negative { 15 } else { 0 };
            let neighbour = seasonal_leaf_chunk(mode, &[(adjacent, 1)]);
            let offset = std::array::from_fn(|test_axis| {
                if test_axis == axis {
                    if negative {
                        -1
                    } else {
                        1
                    }
                } else {
                    0
                }
            });
            let mut neighbourhood = MeshNeighbourhood::new(&center);
            assert!(neighbourhood.insert(offset, &neighbour));
            let mesh = mesh_sub_chunk_in_neighbourhood(&classifier, &assets, mode, &neighbourhood);
            assert_leaf_layer(&mesh, &assets, origin, true);
            let opposite = match face {
                Face::NegativeX => Face::PositiveX,
                Face::PositiveX => Face::NegativeX,
                Face::NegativeY => Face::PositiveY,
                Face::PositiveY => Face::NegativeY,
                Face::NegativeZ => Face::PositiveZ,
                Face::PositiveZ => Face::NegativeZ,
            };
            let mut reverse = MeshNeighbourhood::new(&neighbour);
            assert!(reverse.insert(offset.map(|value| -value), &center));
            let reverse_mesh =
                mesh_sub_chunk_in_neighbourhood(&classifier, &assets, mode, &reverse);
            assert!(
                !reverse_mesh
                    .cube_quads()
                    .iter()
                    .any(|quad| quad.origin() == adjacent && quad.face() == opposite),
                "non-deep neighbour must not overlap the deep surface: {face:?}"
            );
            let open = mesh_sub_chunk_in_neighbourhood(
                &classifier,
                &assets,
                mode,
                &MeshNeighbourhood::new(&center),
            );
            assert_leaf_layer(&open, &assets, origin, false);
        }
    }
}
