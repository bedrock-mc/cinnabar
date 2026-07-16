use std::{mem::size_of, sync::OnceLock};

use assets::{
    Animation, BlockFlags, BlockVisual, CompiledAssets, CompiledBiomeAssets, ContributorRole,
    DIAGNOSTIC_MATERIAL, MATERIAL_FLAG_ALPHA_BLEND, MATERIAL_FLAG_ALPHA_CUTOUT,
    MATERIAL_FLAG_LIQUID_DEPTH_WRITE, MATERIAL_FLAG_WATER_TINT, Material, NO_ANIMATION,
    NO_MODEL_TEMPLATE, NetworkIdMode, RuntimeAssets, TextureArray, TextureMip, TexturePage,
    TextureRef, VisualKind, encode_blob,
};
use meshing::{
    BlockClassifier, Face, LiquidLevel, MeshLightSample, Neighbourhood, PackedLiquidQuad,
    mesh_sub_chunk, mesh_sub_chunk_in_neighbourhood, mesh_sub_chunk_in_neighbourhood_with_lighting,
};
use world::{MeshNeighbourhood, SubChunk};

const AIR: u32 = 0;
const WATER_SOURCE: u32 = 1;
const WATER_ALIAS: u32 = 17;
const SOLID: u32 = 18;
const CROSS: u32 = 19;
const OTHER_LIQUID: u32 = 20;
const FACED_LIQUID: u32 = 21;
const FACED_ALIAS: u32 = 22;
const FACED_DEPTH_7: u32 = 23;
const NON_WATER_LIQUID: u32 = 24;
const GLASS: u32 = 25;
const STILL: u32 = 1;
const FLOW: u32 = 2;

#[test]
fn state_derived_levels_cover_source_flowing_and_falling() {
    let expected = [227, 198, 170, 142, 113, 85, 57, 28];
    for (depth, height) in expected.into_iter().enumerate() {
        let flowing = LiquidLevel::from_variant(depth as u32).expect("bounded liquid depth");
        assert_eq!(flowing.depth(), depth as u8);
        assert_eq!(flowing.height(), height);
        assert!(!flowing.is_falling());

        let falling = LiquidLevel::from_variant(depth as u32 + 8).expect("bounded falling depth");
        assert_eq!(falling.depth(), depth as u8);
        assert_eq!(falling.height(), 227);
        assert!(falling.is_falling());
    }
    assert!(LiquidLevel::from_variant(16).is_none());
}

#[test]
fn packed_schema_preserves_signed_gradient_and_relative_lighting_address() {
    assert_eq!(size_of::<PackedLiquidQuad>(), 16);
    let packed = PackedLiquidQuad::try_pack(
        [3, 4, 5],
        Face::PositiveY,
        [10, 20, 30, 40],
        7,
        11,
        [-17, 23],
        true,
    )
    .expect("bounded packed liquid record");
    assert_eq!(packed.origin(), [3, 4, 5]);
    assert_eq!(packed.face(), Face::PositiveY);
    assert_eq!(packed.heights(), [10, 20, 30, 40]);
    assert_eq!(packed.material_id(), 7);
    assert!(!packed.is_depth_writing());
    assert_eq!(packed.lighting_index(), 11);
    assert_eq!(packed.flow_gradient(), [-17, 23]);
    assert!(packed.is_falling());
    assert!(
        PackedLiquidQuad::try_pack([16, 0, 0], Face::NegativeX, [0; 4], 0, 0, [0, 0], false)
            .is_none()
    );
    assert!(
        PackedLiquidQuad::try_pack(
            [0, 0, 0],
            Face::NegativeX,
            [0; 4],
            1 << 31,
            0,
            [0, 0],
            false,
        )
        .is_none()
    );
    let mut reserved = packed.words();
    reserved[0] = (reserved[0] & !(7 << 12)) | (6 << 12);
    assert!(PackedLiquidQuad::try_from_words(reserved).is_none());
    assert_eq!(
        PackedLiquidQuad::try_from_words(packed.words()),
        Some(packed)
    );
}

#[test]
fn corner_heights_are_weighted_diagonal_gated_and_promoted_by_liquid_above() {
    let isolated = mesh(&blocks(&[(WATER_SOURCE, [8, 8, 8])]));
    assert_eq!(
        quad_at(&isolated, [8, 8, 8], Face::PositiveY).heights(),
        [189; 4]
    );

    let diagonal_only = mesh(&blocks(&[(WATER_SOURCE, [8, 8, 8]), (8, [9, 8, 9])]));
    assert_eq!(
        quad_at(&diagonal_only, [8, 8, 8], Face::PositiveY).heights()[2],
        189
    );

    let gated = mesh(&blocks(&[
        (WATER_SOURCE, [8, 8, 8]),
        (8, [9, 8, 9]),
        (8, [9, 8, 8]),
    ]));
    assert!(quad_at(&gated, [8, 8, 8], Face::PositiveY).heights()[2] < 189);

    let promoted = mesh(&blocks(&[
        (WATER_SOURCE, [8, 8, 8]),
        (WATER_ALIAS, [9, 8, 8]),
        (WATER_ALIAS, [9, 9, 9]),
    ]));
    assert_eq!(
        quad_at(&promoted, [8, 8, 8], Face::PositiveY).heights()[2],
        LiquidLevel::FULL_HEIGHT
    );
}

#[test]
fn liquid_faces_are_clipped_and_culled_by_compatible_liquid_or_solid() {
    let isolated = mesh(&blocks(&[(WATER_SOURCE, [8, 8, 8])]));
    assert_eq!(isolated.liquid_quads().len(), 6);
    assert_eq!(
        quad_at(&isolated, [8, 8, 8], Face::NegativeY).heights(),
        [0; 4]
    );
    for side in [
        Face::NegativeX,
        Face::PositiveX,
        Face::NegativeZ,
        Face::PositiveZ,
    ] {
        let heights = quad_at(&isolated, [8, 8, 8], side).heights();
        assert_eq!(heights.iter().filter(|&&height| height == 189).count(), 2);
        assert_eq!(heights.iter().filter(|&&height| height == 0).count(), 2);
    }

    let aliases = mesh(&blocks(&[
        (WATER_SOURCE, [8, 8, 8]),
        (WATER_ALIAS, [9, 8, 8]),
    ]));
    assert_eq!(aliases.liquid_quads().len(), 10);
    assert!(
        !aliases
            .liquid_quads()
            .iter()
            .any(|quad| quad.origin() == [8, 8, 8] && quad.face() == Face::PositiveX)
    );
    assert!(
        !aliases
            .liquid_quads()
            .iter()
            .any(|quad| quad.origin() == [9, 8, 8] && quad.face() == Face::NegativeX)
    );

    let vertical = mesh(&blocks(&[
        (WATER_SOURCE, [8, 8, 8]),
        (WATER_ALIAS, [8, 9, 8]),
    ]));
    assert!(
        !vertical
            .liquid_quads()
            .iter()
            .any(|quad| quad.origin() == [8, 8, 8] && quad.face() == Face::PositiveY)
    );
    assert!(
        !vertical
            .liquid_quads()
            .iter()
            .any(|quad| quad.origin() == [8, 9, 8] && quad.face() == Face::NegativeY)
    );

    let different = mesh(&blocks(&[
        (WATER_SOURCE, [8, 8, 8]),
        (OTHER_LIQUID, [9, 8, 8]),
    ]));
    assert_eq!(different.liquid_quads().len(), 12);
    let different_above = mesh(&blocks(&[
        (WATER_SOURCE, [8, 8, 8]),
        (OTHER_LIQUID, [8, 9, 8]),
    ]));
    assert!(
        different_above
            .liquid_quads()
            .iter()
            .any(|quad| quad.origin() == [8, 8, 8] && quad.face() == Face::PositiveY)
    );

    let occluded = mesh(&blocks(&[
        (WATER_SOURCE, [8, 8, 8]),
        (SOLID, [8, 9, 8]),
        (SOLID, [9, 8, 8]),
        (SOLID, [8, 7, 8]),
    ]));
    assert_eq!(occluded.liquid_quads().len(), 3);
}

#[test]
fn alpha_glass_enclosure_retains_contacting_water_faces_but_opaque_enclosure_culls() {
    let enclosure = |neighbour| {
        mesh(&blocks(&[
            (WATER_SOURCE, [8, 8, 8]),
            (neighbour, [7, 8, 8]),
            (neighbour, [9, 8, 8]),
            (neighbour, [8, 7, 8]),
            (neighbour, [8, 9, 8]),
            (neighbour, [8, 8, 7]),
            (neighbour, [8, 8, 9]),
        ]))
    };

    assert_eq!(enclosure(GLASS).liquid_quads().len(), 6);
    assert!(enclosure(SOLID).liquid_quads().is_empty());
}

#[test]
fn evidenced_depth_gradient_selects_still_or_flow_material_and_identity() {
    let isolated = mesh(&blocks(&[(WATER_SOURCE, [8, 8, 8])]));
    assert_eq!(
        quad_at(&isolated, [8, 8, 8], Face::PositiveY).flow_gradient(),
        [0, 0]
    );
    assert_eq!(
        quad_at(&isolated, [8, 8, 8], Face::PositiveY).material_id(),
        STILL
    );
    assert_eq!(
        quad_at(&isolated, [8, 8, 8], Face::PositiveX).material_id(),
        FLOW
    );
    assert_eq!(
        quad_at(&isolated, [8, 8, 8], Face::NegativeY).material_id(),
        STILL
    );

    let equal = mesh(&blocks(&[
        (WATER_SOURCE, [8, 8, 8]),
        (WATER_ALIAS, [9, 8, 8]),
    ]));
    assert_eq!(
        quad_at(&equal, [8, 8, 8], Face::PositiveY).flow_gradient(),
        [0, 0]
    );

    let asymmetric = mesh(&blocks(&[(WATER_SOURCE, [8, 8, 8]), (8, [9, 8, 8])]));
    let top = quad_at(&asymmetric, [8, 8, 8], Face::PositiveY);
    assert_eq!(top.flow_gradient(), [7, 0]);
    assert_eq!(top.material_id(), FLOW);

    let downward = mesh(&blocks(&[
        (WATER_SOURCE, [8, 1, 8]),
        (WATER_SOURCE, [9, 0, 8]),
    ]));
    assert_eq!(
        quad_at(&downward, [8, 1, 8], Face::PositiveY).flow_gradient(),
        [8, 0]
    );

    let falling = mesh(&blocks(&[(9, [8, 8, 8])]));
    let falling_top = quad_at(&falling, [8, 8, 8], Face::PositiveY);
    assert!(falling_top.is_falling());
    assert_eq!(falling_top.flow_gradient(), [0, 0]);
    assert_eq!(falling_top.material_id(), STILL);

    let still = runtime_assets().material(STILL);
    let flow = runtime_assets().material(FLOW);
    assert_ne!(still.animation, flow.animation);
    for material in [still, flow] {
        assert_eq!(
            material.flags & (MATERIAL_FLAG_ALPHA_BLEND | MATERIAL_FLAG_WATER_TINT),
            MATERIAL_FLAG_ALPHA_BLEND | MATERIAL_FLAG_WATER_TINT
        );
    }
}

#[test]
fn lower_cardinal_cross_subchunk_sample_drives_boundary_flow() {
    let center = blocks(&[(WATER_SOURCE, [15, 0, 8])]);
    let lower_east = blocks(&[(WATER_SOURCE, [0, 15, 8])]);
    let mut neighbourhood = MeshNeighbourhood::new(&center);
    assert!(neighbourhood.insert([1, -1, 0], &lower_east));
    let mesh = mesh_sub_chunk_in_neighbourhood(
        &BlockClassifier::new(AIR),
        runtime_assets(),
        NetworkIdMode::Sequential,
        &neighbourhood,
    );
    assert_eq!(
        quad_at(&mesh, [15, 0, 8], Face::PositiveY).flow_gradient(),
        [8, 0]
    );
}

#[test]
fn lower_cardinal_flow_preserves_all_signed_axes_and_negative_seams() {
    for (below, expected) in [
        ([7, 0, 8], [-8, 0]),
        ([8, 0, 7], [0, -8]),
        ([8, 0, 9], [0, 8]),
    ] {
        let mesh = mesh(&blocks(&[(WATER_SOURCE, [8, 1, 8]), (WATER_SOURCE, below)]));
        assert_eq!(
            quad_at(&mesh, [8, 1, 8], Face::PositiveY).flow_gradient(),
            expected
        );
    }

    for (current, offset, remote, expected) in [
        ([0, 0, 8], [-1, -1, 0], [15, 15, 8], [-8, 0]),
        ([8, 0, 0], [0, -1, -1], [8, 15, 15], [0, -8]),
    ] {
        let center = blocks(&[(WATER_SOURCE, current)]);
        let neighbour = blocks(&[(WATER_SOURCE, remote)]);
        let mut neighbourhood = MeshNeighbourhood::new(&center);
        assert!(neighbourhood.insert(offset, &neighbour));
        let mesh = mesh_sub_chunk_in_neighbourhood(
            &BlockClassifier::new(AIR),
            runtime_assets(),
            NetworkIdMode::Sequential,
            &neighbourhood,
        );
        assert_eq!(
            quad_at(&mesh, current, Face::PositiveY).flow_gradient(),
            expected
        );
    }
}

#[test]
fn falling_flow_uses_effective_zero_depth_while_retaining_raw_state_depth() {
    let source = mesh(&blocks(&[(WATER_SOURCE, [8, 8, 8]), (2, [9, 8, 8])]));
    let falling = mesh(&blocks(&[(16, [8, 8, 8]), (2, [9, 8, 8])]));
    assert_eq!(LiquidLevel::from_variant(15).unwrap().depth(), 7);
    assert_eq!(
        quad_at(&source, [8, 8, 8], Face::PositiveY).flow_gradient(),
        [1, 0]
    );
    assert_eq!(
        quad_at(&falling, [8, 8, 8], Face::PositiveY).flow_gradient(),
        [1, 0]
    );
}

#[test]
fn waterlogging_retains_model_and_exactly_one_lighting_record_per_liquid_quad() {
    let center = layered(CROSS, WATER_SOURCE, [8, 8, 8]);
    let mesh = mesh(&center);
    assert_eq!(mesh.model_refs().len(), 1);
    assert_eq!(mesh.liquid_quads().len(), 6);
    assert_eq!(mesh.liquid_lighting().len(), mesh.liquid_quads().len());
    for (index, (quad, lighting)) in mesh
        .liquid_quads()
        .iter()
        .zip(mesh.liquid_lighting())
        .enumerate()
    {
        assert_eq!(quad.lighting_index(), index as u32);
        assert!(
            lighting
                .samples()
                .into_iter()
                .all(|sample| sample & !0x03ff == 0)
        );
    }
}

#[test]
fn liquid_lighting_is_face_specific_and_ao_samples_the_expected_corner() {
    let isolated = mesh(&blocks(&[(WATER_SOURCE, [8, 8, 8])]));
    let top = quad_at(&isolated, [8, 8, 8], Face::PositiveY);
    assert_eq!(
        isolated.liquid_lighting()[top.lighting_index() as usize].samples(),
        [0x00f0; 4]
    );

    let occluded_corner = mesh(&blocks(&[(WATER_SOURCE, [8, 8, 8]), (SOLID, [7, 9, 7])]));
    let top = quad_at(&occluded_corner, [8, 8, 8], Face::PositiveY);
    assert_eq!(
        occluded_corner.liquid_lighting()[top.lighting_index() as usize].samples(),
        [0x01f0, 0x00f0, 0x00f0, 0x00f0]
    );
}

#[test]
fn liquid_lighting_uses_the_render_owned_sampler() {
    let center = blocks(&[(WATER_SOURCE, [8, 8, 8])]);
    let neighbourhood = MeshNeighbourhood::new(&center);
    let sampler = |_coordinate: [i32; 3]| MeshLightSample::try_new(11, 3).unwrap();
    let mesh = mesh_sub_chunk_in_neighbourhood_with_lighting(
        &BlockClassifier::new(AIR),
        runtime_assets(),
        NetworkIdMode::Sequential,
        &neighbourhood,
        &sampler,
    );

    assert!(!mesh.liquid_lighting().is_empty());
    assert!(
        mesh.liquid_lighting()
            .iter()
            .flat_map(|lighting| lighting.samples())
            .all(|sample| sample & 0x00ff == 0x003b)
    );
}

#[test]
fn side_and_bottom_lighting_indices_match_the_packed_vertex_winding() {
    for (face, occluder, expected_index) in [
        (Face::NegativeX, [7, 9, 7], 1_usize),
        (Face::PositiveX, [9, 9, 9], 1),
        (Face::NegativeZ, [9, 9, 7], 1),
        (Face::PositiveZ, [7, 9, 9], 1),
        (Face::NegativeY, [7, 7, 7], 0),
    ] {
        let mesh = mesh(&blocks(&[(WATER_SOURCE, [8, 8, 8]), (SOLID, occluder)]));
        let quad = quad_at(&mesh, [8, 8, 8], face);
        let samples = mesh.liquid_lighting()[quad.lighting_index() as usize].samples();
        let mut expected = [0x00f0; 4];
        expected[expected_index] = 0x01f0;
        assert_eq!(samples, expected, "{face:?} packed/lighting vertex order");
    }
}

#[test]
fn liquid_preserves_all_compiled_face_materials_separately_from_compatibility() {
    let isolated = mesh(&blocks(&[(FACED_LIQUID, [8, 8, 8])]));
    for (face, material) in [
        (Face::NegativeX, 7),
        (Face::PositiveX, 8),
        (Face::NegativeY, 9),
        (Face::PositiveY, 10),
        (Face::NegativeZ, 11),
        (Face::PositiveZ, 12),
    ] {
        assert_eq!(quad_at(&isolated, [8, 8, 8], face).material_id(), material);
    }
    let animations = (7..=12)
        .map(|material| runtime_assets().material(material).animation)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(animations.len(), 6);

    let flowing = mesh(&blocks(&[
        (FACED_LIQUID, [8, 8, 8]),
        (FACED_DEPTH_7, [9, 8, 8]),
    ]));
    assert_eq!(
        quad_at(&flowing, [8, 8, 8], Face::PositiveY).material_id(),
        7
    );

    let aliases = mesh(&blocks(&[
        (FACED_LIQUID, [8, 8, 8]),
        (FACED_ALIAS, [9, 8, 8]),
    ]));
    assert!(
        !aliases
            .liquid_quads()
            .iter()
            .any(|quad| quad.origin() == [8, 8, 8] && quad.face() == Face::PositiveX)
    );
}

#[test]
fn depth_writing_lava_uses_the_shared_liquid_stream_without_water_flags() {
    let mesh = mesh(&blocks(&[(NON_WATER_LIQUID, [8, 8, 8])]));
    assert_eq!(mesh.liquid_quads().len(), 6);
    assert_eq!(mesh.liquid_lighting().len(), 6);
    assert!(mesh.liquid_quads().iter().all(|quad| {
        let flags = runtime_assets().material(quad.material_id()).flags;
        quad.is_depth_writing()
            && flags & MATERIAL_FLAG_LIQUID_DEPTH_WRITE != 0
            && flags & (MATERIAL_FLAG_ALPHA_BLEND | MATERIAL_FLAG_WATER_TINT) == 0
    }));
}

#[test]
fn mixed_water_and_lava_are_stably_partitioned_with_both_interface_faces() {
    let mesh = mesh(&blocks(&[
        (WATER_SOURCE, [8, 8, 8]),
        (NON_WATER_LIQUID, [9, 8, 8]),
    ]));
    let split = mesh
        .liquid_quads()
        .iter()
        .position(|quad| quad.is_depth_writing())
        .expect("lava suffix");
    assert_eq!(split, 6);
    assert_eq!(mesh.liquid_quads().len(), 12);
    assert!(
        mesh.liquid_quads()[..split]
            .iter()
            .all(|quad| !quad.is_depth_writing())
    );
    assert!(
        mesh.liquid_quads()[split..]
            .iter()
            .all(|quad| quad.is_depth_writing())
    );
    assert!(
        mesh.liquid_quads()
            .iter()
            .any(|quad| { quad.origin() == [8, 8, 8] && quad.face() == Face::PositiveX })
    );
    assert!(
        mesh.liquid_quads()
            .iter()
            .any(|quad| { quad.origin() == [9, 8, 8] && quad.face() == Face::NegativeX })
    );
    assert!(
        mesh.liquid_quads()
            .iter()
            .enumerate()
            .all(|(index, quad)| quad.lighting_index() == index as u32)
    );
}

#[test]
fn depth_writing_lava_culls_matching_faces_across_all_subchunk_boundaries() {
    for (offset, local, remote, face) in [
        ([-1, 0, 0], [0, 8, 8], [15, 8, 8], Face::NegativeX),
        ([1, 0, 0], [15, 8, 8], [0, 8, 8], Face::PositiveX),
        ([0, -1, 0], [8, 0, 8], [8, 15, 8], Face::NegativeY),
        ([0, 1, 0], [8, 15, 8], [8, 0, 8], Face::PositiveY),
        ([0, 0, -1], [8, 8, 0], [8, 8, 15], Face::NegativeZ),
        ([0, 0, 1], [8, 8, 15], [8, 8, 0], Face::PositiveZ),
    ] {
        let center = blocks(&[(NON_WATER_LIQUID, local)]);
        let adjacent = blocks(&[(NON_WATER_LIQUID, remote)]);
        let mut neighbourhood = MeshNeighbourhood::new(&center);
        assert!(neighbourhood.insert(offset, &adjacent));
        let mesh = mesh_sub_chunk_in_neighbourhood(
            &BlockClassifier::new(AIR),
            runtime_assets(),
            NetworkIdMode::Sequential,
            &neighbourhood,
        );
        assert!(
            !mesh
                .liquid_quads()
                .iter()
                .any(|quad| quad.origin() == local && quad.face() == face),
            "shared lava face survived at boundary {offset:?}",
        );
    }
}

#[test]
fn legacy_six_neighbour_api_is_explicitly_cube_model_only() {
    let center = blocks(&[(WATER_SOURCE, [8, 8, 8])]);
    let legacy = mesh_sub_chunk(
        &BlockClassifier::new(AIR),
        runtime_assets(),
        NetworkIdMode::Sequential,
        &Neighbourhood::default(),
        &center,
    );
    assert!(legacy.liquid_quads().is_empty());
    assert!(legacy.liquid_lighting().is_empty());
    assert_eq!(mesh(&center).liquid_quads().len(), 6);
}

#[test]
fn liquid_stream_order_is_xyz_then_stable_face_order_with_relative_lighting() {
    let mesh = mesh(&blocks(&[
        (WATER_SOURCE, [2, 3, 4]),
        (WATER_SOURCE, [1, 5, 6]),
    ]));
    let expected_faces = [
        Face::PositiveY,
        Face::NegativeX,
        Face::PositiveX,
        Face::NegativeZ,
        Face::PositiveZ,
        Face::NegativeY,
    ];
    assert_eq!(
        mesh.liquid_quads()[..6]
            .iter()
            .map(|quad| (quad.origin(), quad.face()))
            .collect::<Vec<_>>(),
        expected_faces.map(|face| ([1, 5, 6], face))
    );
    assert_eq!(
        mesh.liquid_quads()[6..]
            .iter()
            .map(|quad| (quad.origin(), quad.face()))
            .collect::<Vec<_>>(),
        expected_faces.map(|face| ([2, 3, 4], face))
    );
    assert!(
        mesh.liquid_quads()
            .iter()
            .enumerate()
            .all(|(index, quad)| quad.lighting_index() == index as u32)
    );
}

#[test]
fn corner_and_top_culling_cross_positive_subchunk_seams() {
    let center = blocks(&[(WATER_SOURCE, [15, 8, 15])]);
    let east = blocks(&[(8, [0, 8, 15])]);
    let south = blocks(&[(8, [15, 8, 0])]);
    let south_east = blocks(&[(8, [0, 8, 0])]);
    let mut neighbourhood = MeshNeighbourhood::new(&center);
    assert!(neighbourhood.insert([1, 0, 0], &east));
    assert!(neighbourhood.insert([0, 0, 1], &south));
    assert!(neighbourhood.insert([1, 0, 1], &south_east));
    let mesh = mesh_sub_chunk_in_neighbourhood(
        &BlockClassifier::new(AIR),
        runtime_assets(),
        NetworkIdMode::Sequential,
        &neighbourhood,
    );
    let heights = quad_at(&mesh, [15, 8, 15], Face::PositiveY).heights();
    assert!(heights[2] < heights[0]);

    let top_center = blocks(&[(WATER_SOURCE, [8, 15, 8])]);
    let above = blocks(&[(WATER_ALIAS, [8, 0, 8])]);
    let mut neighbourhood = MeshNeighbourhood::new(&top_center);
    assert!(neighbourhood.insert([0, 1, 0], &above));
    let mesh = mesh_sub_chunk_in_neighbourhood(
        &BlockClassifier::new(AIR),
        runtime_assets(),
        NetworkIdMode::Sequential,
        &neighbourhood,
    );
    assert!(
        !mesh
            .liquid_quads()
            .iter()
            .any(|quad| quad.origin() == [8, 15, 8] && quad.face() == Face::PositiveY)
    );
}

#[test]
fn corner_height_crosses_negative_x_z_and_diagonal_seams() {
    let center = blocks(&[(WATER_SOURCE, [0, 8, 0])]);
    let west = blocks(&[(8, [15, 8, 0])]);
    let north = blocks(&[(8, [0, 8, 15])]);
    let north_west = blocks(&[(8, [15, 8, 15])]);
    let mut neighbourhood = MeshNeighbourhood::new(&center);
    assert!(neighbourhood.insert([-1, 0, 0], &west));
    assert!(neighbourhood.insert([0, 0, -1], &north));
    assert!(neighbourhood.insert([-1, 0, -1], &north_west));
    let mesh = mesh_sub_chunk_in_neighbourhood(
        &BlockClassifier::new(AIR),
        runtime_assets(),
        NetworkIdMode::Sequential,
        &neighbourhood,
    );
    let heights = quad_at(&mesh, [0, 8, 0], Face::PositiveY).heights();
    assert!(heights[0] < heights[2]);
}

fn quad_at(mesh: &meshing::ChunkMesh, origin: [u8; 3], face: Face) -> PackedLiquidQuad {
    *mesh
        .liquid_quads()
        .iter()
        .find(|quad| quad.origin() == origin && quad.face() == face)
        .expect("liquid quad")
}

fn mesh(center: &SubChunk) -> meshing::ChunkMesh {
    let neighbourhood = MeshNeighbourhood::new(center);
    mesh_sub_chunk_in_neighbourhood(
        &BlockClassifier::new(AIR),
        runtime_assets(),
        NetworkIdMode::Sequential,
        &neighbourhood,
    )
}

fn runtime_assets() -> &'static RuntimeAssets {
    static ASSETS: OnceLock<RuntimeAssets> = OnceLock::new();
    ASSETS.get_or_init(|| {
        let diagnostic = BlockVisual {
            faces: [DIAGNOSTIC_MATERIAL; 6],
            flags: BlockFlags::empty(),
            kind: VisualKind::Diagnostic,
            contributor_role: ContributorRole::Primary,
            model_template: NO_MODEL_TEMPLATE,
            animation: NO_ANIMATION,
            variant: 0,
        };
        let mut visuals = vec![diagnostic; 26];
        visuals[AIR as usize] = BlockVisual {
            flags: BlockFlags::AIR,
            kind: VisualKind::Invisible,
            contributor_role: ContributorRole::Air,
            ..diagnostic
        };
        for depth in 0..16 {
            visuals[depth + 1] =
                liquid_visual([FLOW, FLOW, STILL, STILL, FLOW, FLOW], depth as u32);
        }
        visuals[WATER_ALIAS as usize] = liquid_visual([FLOW, FLOW, STILL, STILL, FLOW, FLOW], 0);
        visuals[OTHER_LIQUID as usize] = liquid_visual([4, 4, 3, 3, 4, 4], 0);
        let faced = [7, 8, 9, 10, 11, 12];
        visuals[FACED_LIQUID as usize] = liquid_visual(faced, 0);
        visuals[FACED_ALIAS as usize] = liquid_visual(faced, 0);
        visuals[FACED_DEPTH_7 as usize] = liquid_visual(faced, 7);
        visuals[NON_WATER_LIQUID as usize] = liquid_visual([14, 14, 13, 13, 14, 14], 0);
        visuals[SOLID as usize] = BlockVisual {
            faces: [5; 6],
            flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
            kind: VisualKind::Cube,
            ..diagnostic
        };
        visuals[CROSS as usize] = BlockVisual {
            faces: [6; 6],
            kind: VisualKind::Cross,
            model_template: 0,
            ..diagnostic
        };
        visuals[GLASS as usize] = BlockVisual {
            faces: [15; 6],
            flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
            kind: VisualKind::Cube,
            ..diagnostic
        };
        let mut materials = vec![
            Material {
                texture: TextureRef::DIAGNOSTIC,
                flags: 0,
                animation: NO_ANIMATION
            };
            16
        ];
        materials[STILL as usize] = Material {
            texture: TextureRef::new(0, 0).unwrap(),
            flags: MATERIAL_FLAG_ALPHA_BLEND | MATERIAL_FLAG_WATER_TINT,
            animation: 0,
        };
        materials[FLOW as usize] = Material {
            texture: TextureRef::new(0, 1).unwrap(),
            flags: MATERIAL_FLAG_ALPHA_BLEND | MATERIAL_FLAG_WATER_TINT,
            animation: 1,
        };
        materials[15] = Material {
            texture: TextureRef::new(0, 0).unwrap(),
            flags: MATERIAL_FLAG_ALPHA_CUTOUT,
            animation: NO_ANIMATION,
        };
        for material in [13_usize, 14] {
            materials[material] = Material {
                texture: TextureRef::new(0, (material - 13) as u32).unwrap(),
                flags: MATERIAL_FLAG_LIQUID_DEPTH_WRITE,
                animation: (material - 13) as u32,
            };
        }
        for (material, animation) in [(3_usize, 0_u32), (4, 1)] {
            materials[material] = Material {
                texture: TextureRef::new(0, animation).unwrap(),
                flags: MATERIAL_FLAG_ALPHA_BLEND | MATERIAL_FLAG_WATER_TINT,
                animation,
            };
        }
        for (material, entry) in materials.iter_mut().enumerate().take(13).skip(7) {
            *entry = Material {
                texture: TextureRef::new(0, (material - 5) as u32).unwrap(),
                flags: MATERIAL_FLAG_ALPHA_BLEND | MATERIAL_FLAG_WATER_TINT,
                animation: (material - 5) as u32,
            };
        }
        let animations = (0..8)
            .map(|frame| Animation {
                frame_start: frame,
                frame_count: 1,
                ticks_per_frame: 1,
                atlas_index: 0,
                atlas_tile_variant: 0,
                replicate: 1,
                flags: 0,
            })
            .collect::<Vec<_>>();
        let textures = TextureArray {
            layers: 8,
            mips: [16_u32, 8, 4, 2, 1]
                .into_iter()
                .map(|size| TextureMip {
                    size,
                    rgba8: vec![255; size as usize * size as usize * 4 * 8].into_boxed_slice(),
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        };
        let light_properties = vec![assets::LightProperties::default(); visuals.len()];
        let compiled = CompiledAssets {
            visuals: visuals.into_boxed_slice(),
            hashed: Box::new([]),
            materials: materials.into_boxed_slice(),
            model_templates: vec![assets::ModelTemplate {
                quad_start: 0,
                quad_count: 1,
                flags: 0,
            }]
            .into_boxed_slice(),
            light_properties: light_properties.into_boxed_slice(),
            model_quads: vec![assets::ModelQuad {
                positions: [[0, 0, 0], [256, 0, 256], [256, 256, 256], [0, 256, 0]],
                uvs: [[0, 0]; 4],
                material: 6,
                flags: assets::MODEL_QUAD_FLAG_TWO_SIDED,
            }]
            .into_boxed_slice(),
            animations: animations.into_boxed_slice(),
            animation_frames: (0..8)
                .map(|layer| TextureRef::new(0, layer).unwrap())
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            texture_pages: vec![TexturePage::new(textures)].into_boxed_slice(),
            biomes: CompiledBiomeAssets::diagnostic(),
        };
        RuntimeAssets::decode(&encode_blob(&compiled).unwrap()).unwrap()
    })
}

fn liquid_visual(faces: [u32; 6], variant: u32) -> BlockVisual {
    BlockVisual {
        faces,
        flags: BlockFlags::empty(),
        kind: VisualKind::Liquid,
        contributor_role: ContributorRole::LiquidAdditional,
        model_template: NO_MODEL_TEMPLATE,
        animation: NO_ANIMATION,
        variant,
    }
}

fn blocks(entries: &[(u32, [u8; 3])]) -> SubChunk {
    let mut palette = vec![AIR];
    let placements = entries
        .iter()
        .map(|&(id, pos)| {
            let index = palette
                .iter()
                .position(|&value| value == id)
                .unwrap_or_else(|| {
                    palette.push(id);
                    palette.len() - 1
                });
            (pos, index)
        })
        .collect::<Vec<_>>();
    sub_chunk(vec![packed_storage(5, &palette, &placements)])
}
fn layered(primary: u32, liquid: u32, position: [u8; 3]) -> SubChunk {
    sub_chunk(vec![
        packed_storage(1, &[AIR, primary], &[(position, 1)]),
        packed_storage(1, &[AIR, liquid], &[(position, 1)]),
    ])
}
fn packed_storage(bits: u8, palette: &[u32], placements: &[([u8; 3], usize)]) -> Vec<u8> {
    let per = 32 / usize::from(bits);
    let mut words = vec![0_u32; 4096_usize.div_ceil(per)];
    for &([x, y, z], index) in placements {
        let linear = (usize::from(x) << 8) | (usize::from(z) << 4) | usize::from(y);
        words[linear / per] |= (index as u32) << ((linear % per) * usize::from(bits));
    }
    let mut out = vec![(bits << 1) | 1];
    for word in words {
        out.extend(word.to_le_bytes());
    }
    out.extend(varint((palette.len() as i32) << 1));
    for &id in palette {
        out.extend(varint((id as i32) << 1));
    }
    out
}
fn sub_chunk(storages: Vec<Vec<u8>>) -> SubChunk {
    let mut out = vec![9, storages.len() as u8, 0];
    for storage in storages {
        out.extend(storage);
    }
    SubChunk::decode(&out).unwrap()
}
fn varint(mut value: i32) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80
        }
        out.push(byte);
        if value == 0 {
            return out;
        }
    }
}
