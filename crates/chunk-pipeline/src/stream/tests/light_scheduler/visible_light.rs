use super::*;
use crate::stream::lighting::VisibleLightChange;

const AIR: u32 = 0;
const EMITTING_CUBE: u32 = 1;
const SOLID_CUBE: u32 = 2;
const OPEN_CUBE: u32 = 3;

/// Judges a first solve of `light` over a section filled with `block`, or none resident.
fn first_solve(block: Option<u32>, light: &SubChunkLight) -> VisibleLightChange {
    let mut stream = lit_stream(0);
    let key = SubChunkKey::new(0, 0, 5, 0);
    if let Some(block) = block {
        stream
            .authority
            .commit_sub_chunk(key, super::uniform_sub_chunk(block))
            .unwrap();
        stream.sync_resident(key);
    }
    stream.first_light_visible_change(key, light)
}

fn unchanged() -> VisibleLightChange {
    VisibleLightChange {
        interior: false,
        faces: [false; 6],
    }
}

#[test]
fn first_light_counts_only_cells_a_provisional_mesh_could_read() {
    let dark = SubChunkLight::dark(1);
    let sky = SubChunkLight::uniform(0, 15, 1).unwrap();
    // Dark solid blocks and open sky match what the default-lit mesh already drew.
    assert_eq!(first_solve(Some(SOLID_CUBE), &dark), unchanged());
    assert_eq!(first_solve(Some(AIR), &sky), unchanged());
    assert_eq!(first_solve(None, &sky), unchanged());
    // Open cells, non-occluding cubes and emitters are read, so their light counts.
    for block in [Some(AIR), Some(OPEN_CUBE), Some(EMITTING_CUBE), None] {
        let change = first_solve(block, &dark);
        assert!(
            change.interior && change.faces.iter().all(|&face| face),
            "{block:?}"
        );
    }
    // A dimmer open cell on the -X boundary reaches only that neighbour.
    let mut shaded = SubChunkLight::uniform(0, 15, 1).unwrap();
    shaded.set(LightChannel::Sky, 0, 5, 5, 14).unwrap();
    assert_eq!(
        first_solve(Some(AIR), &shaded),
        VisibleLightChange {
            interior: true,
            faces: [true, false, false, false, false, false],
        }
    );
}
