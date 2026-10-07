// Current liquid tessellation owns its lighting,
// independently of terrain ambient occlusion sampling.
use std::cell::Cell;

use super::{LightingInputs, MeshLightSample, bake_liquid_quad, bake_quad, cube_face_positions};
use crate::Face;

struct LiquidFaceSamples {
    outward: [i32; 3],
    sample_calls: Cell<usize>,
    occlusion_calls: Cell<usize>,
}

impl LightingInputs for LiquidFaceSamples {
    fn occludes(&self, _coordinate: [i32; 3]) -> bool {
        self.occlusion_calls.set(self.occlusion_calls.get() + 1);
        true
    }

    fn sample(&self, coordinate: [i32; 3]) -> MeshLightSample {
        self.sample_calls.set(self.sample_calls.get() + 1);
        if coordinate == self.outward {
            MeshLightSample::try_new(3, 5).unwrap()
        } else {
            MeshLightSample::try_new(15, 15).unwrap()
        }
    }
}

#[test]
fn liquid_sides_and_bottom_repeat_only_the_outward_light_sample() {
    for (face, outward) in [
        (Face::NegativeX, [-1, 0, 0]),
        (Face::PositiveX, [1, 0, 0]),
        (Face::NegativeY, [0, -1, 0]),
        (Face::NegativeZ, [0, 0, -1]),
        (Face::PositiveZ, [0, 0, 1]),
    ] {
        let inputs = LiquidFaceSamples {
            outward,
            sample_calls: Cell::new(0),
            occlusion_calls: Cell::new(0),
        };
        let result = bake_liquid_quad(&inputs, [0; 3], face, cube_face_positions(face));
        assert_eq!(result.samples(), [0x0053; 4], "{face:?}");
        assert_eq!(inputs.sample_calls.get(), 1, "{face:?}");
        assert_eq!(inputs.occlusion_calls.get(), 0, "{face:?}");
    }
}

#[test]
fn liquid_top_does_not_inherit_terrain_ambient_occlusion() {
    let inputs = LiquidFaceSamples {
        outward: [0, 1, 0],
        sample_calls: Cell::new(0),
        occlusion_calls: Cell::new(0),
    };
    let positions = cube_face_positions(Face::PositiveY);
    let liquid = bake_liquid_quad(&inputs, [0; 3], Face::PositiveY, positions);
    assert_eq!(liquid.samples(), [0x00ff; 4]);

    // Removing liquid AO must not change the accepted terrain-light path.
    let terrain = bake_quad(&inputs, [0; 3], Face::PositiveY, positions, false);
    assert_eq!(terrain.samples(), [0x04ff; 4]);
}
