// Ambient occlusion moves center light outwards only when the center's
// solid-render bit is set;
// the independent boundary flag moves the tangential AO/light sampling plane.
use super::{LightingInputs, MeshLightSample, bake_quad_with};
use crate::Face;

struct NativeFacePlanes {
    own_solid: bool,
    outward_solid: bool,
}

impl LightingInputs for NativeFacePlanes {
    fn occludes(&self, coordinate: [i32; 3]) -> bool {
        (coordinate == [0, 0, 0] && self.own_solid)
            || (coordinate == [0, 1, 0] && self.outward_solid)
    }

    fn sample(&self, coordinate: [i32; 3]) -> MeshLightSample {
        let level = match coordinate {
            [0, 0, 0] => 7,
            [0, 1, 0] => 15,
            [_, 0, _] => 11,
            _ => 2,
        };
        MeshLightSample::try_new(level, 0).unwrap()
    }
}

#[test]
fn native_boundary_model_center_light_uses_the_own_solid_bit() {
    for outward_solid in [false, true] {
        let inputs = NativeFacePlanes {
            own_solid: false,
            outward_solid,
        };
        let result = bake_quad_with(
            &inputs,
            [0, 0, 0],
            Face::PositiveY,
            [[256, 256, 256]; 4],
            true,
            false,
        );
        assert_eq!(result.samples()[0] & 15, 7, "outward solid={outward_solid}");
    }
}

#[test]
fn native_boundary_solid_center_and_tangential_light_stay_outward() {
    let inputs = NativeFacePlanes {
        own_solid: true,
        outward_solid: true,
    };
    let result = bake_quad_with(
        &inputs,
        [0, 0, 0],
        Face::PositiveY,
        [[256, 256, 256]; 4],
        true,
        false,
    );
    assert_eq!(result.samples()[0] & 15, 15);
    assert_eq!((result.samples()[0] >> 8) & 7, 1);
}

#[test]
fn native_inset_face_keeps_center_and_tangential_light_in_the_own_plane() {
    let inputs = NativeFacePlanes {
        own_solid: true,
        outward_solid: true,
    };
    let result = bake_quad_with(
        &inputs,
        [0, 0, 0],
        Face::PositiveY,
        [[256, 128, 256]; 4],
        true,
        false,
    );
    assert_eq!(result.samples()[0] & 15, 11);
    assert_eq!((result.samples()[0] >> 8) & 7, 1);
}

#[test]
fn liquid_admission_retains_the_separate_outward_center_sample() {
    let inputs = NativeFacePlanes {
        own_solid: false,
        outward_solid: false,
    };
    let result = super::bake_liquid_quad(&inputs, [0, 0, 0], Face::PositiveY, [[256, 256, 256]; 4]);
    assert_eq!(result.samples()[0] & 15, 15);
}
