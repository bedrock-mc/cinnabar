use super::{
    AIR, CROSS, GLASS, NON_WATER_LIQUID, OTHER_LIQUID, SOLID, WATER_SOURCE, blocks, mesh,
    packed_storage, sub_chunk,
};
use meshing::{ChunkMesh, Face};

const ORIGIN: [u8; 3] = [8, 8, 8];

fn has_face(mesh: &ChunkMesh, face: Face) -> bool {
    mesh.liquid_quads()
        .iter()
        .any(|quad| quad.origin() == ORIGIN && quad.face() == face)
}

// Vanilla classic WATER material 5 compares neighbouring primary
// BlockType with Air, not its opacity. Deferred model 1 instead compares material;
// non-water liquids do not enter this special gate. Top admission is separate.
#[test]
fn classic_water_hides_non_air_primary_contacts_even_without_opaque_faces() {
    for neighbour in [GLASS, CROSS, OTHER_LIQUID, NON_WATER_LIQUID, SOLID] {
        for (face, adjacent) in [
            (Face::NegativeX, [7, 8, 8]),
            (Face::PositiveX, [9, 8, 8]),
            (Face::NegativeZ, [8, 8, 7]),
            (Face::PositiveZ, [8, 8, 9]),
            (Face::NegativeY, [8, 7, 8]),
        ] {
            let output = mesh(&blocks(&[(WATER_SOURCE, ORIGIN), (neighbour, adjacent)]));
            assert!(!has_face(&output, face), "contact {neighbour} {face:?}");
            assert!(has_face(&output, Face::PositiveY));
            assert_eq!(output.liquid_lighting().len(), output.liquid_quads().len());
            assert!(
                output
                    .liquid_quads()
                    .iter()
                    .enumerate()
                    .all(|(index, quad)| quad.lighting_index() == index as u32)
            );
        }
    }
}

#[test]
fn non_water_liquid_keeps_single_winding_transparent_primary_contacts() {
    for neighbour in [GLASS, CROSS, OTHER_LIQUID, WATER_SOURCE] {
        let output = mesh(&blocks(&[
            (NON_WATER_LIQUID, ORIGIN),
            (neighbour, [9, 8, 8]),
            (neighbour, [8, 7, 8]),
        ]));
        for face in [Face::PositiveX, Face::NegativeY] {
            let quad = output
                .liquid_quads()
                .iter()
                .find(|quad| quad.origin() == ORIGIN && quad.face() == face)
                .expect("native non-water contact face");
            assert!(quad.is_depth_writing());
            assert!(!quad.is_two_sided());
        }
    }
}

#[test]
fn primary_water_and_additional_water_do_not_duplicate_exposed_surfaces() {
    let position = (ORIGIN, 1);
    let output = mesh(&sub_chunk(vec![
        packed_storage(1, &[AIR, WATER_SOURCE], &[position]),
        packed_storage(1, &[AIR, WATER_SOURCE], &[position]),
    ]));
    assert_eq!(output.liquid_quads().len(), Face::ALL.len());
    for face in Face::ALL {
        assert!(has_face(&output, face));
    }
}

#[test]
fn neighbour_primary_controls_contact_admission_not_its_additional_water() {
    for primary in [AIR, GLASS, CROSS] {
        let neighbour = ([9, 8, 8], 1);
        let output = mesh(&sub_chunk(vec![
            packed_storage(2, &[AIR, primary], &[neighbour]),
            packed_storage(
                2,
                &[AIR, WATER_SOURCE, OTHER_LIQUID],
                &[(ORIGIN, 1), ([9, 8, 8], 2)],
            ),
        ]));
        assert_eq!(has_face(&output, Face::PositiveX), primary == AIR);
        assert!(has_face(&output, Face::NegativeX));
        assert!(has_face(&output, Face::PositiveY));
    }
}
