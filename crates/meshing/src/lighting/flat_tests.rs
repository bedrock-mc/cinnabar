use super::*;

/// Reports distinct own and outward light and counts all work needed for a flat face.
struct FlatInputs {
    samples: Cell<usize>,
}

impl LightingInputs for FlatInputs {
    fn smooth_lighting(&self) -> bool {
        false
    }

    fn occludes(&self, _: [i32; 3]) -> bool {
        panic!("flat lighting must not gather AO neighbours")
    }

    fn sample(&self, coordinate: [i32; 3]) -> MeshLightSample {
        self.samples.set(self.samples.get() + 1);
        let level = if coordinate == [0; 3] { 3 } else { 12 };
        MeshLightSample::try_new(level, 7).unwrap()
    }
}

#[test]
fn flat_lighting_samples_once_without_ao_and_preserves_emission() {
    for face in Face::ALL {
        let (normal, _, _) = face_basis(face);
        let axis = normal.iter().position(|&value| value != 0).unwrap();
        for boundary in [false, true] {
            let mut positions = [[128; 3]; 4];
            if boundary {
                for position in &mut positions {
                    position[axis] = if normal[axis] < 0 { 0 } else { 256 };
                }
            }
            for emitting in [false, true] {
                let inputs = FlatInputs {
                    samples: Cell::new(0),
                };
                let actual = bake_quad_with(&inputs, [0; 3], face, positions, true, emitting);
                let expected =
                    pack_sample(if boundary { 12 } else { 3 }, 7, 0) | (u16::from(emitting) << 11);
                assert_eq!(
                    actual.samples(),
                    [expected; 4],
                    "{face:?}, boundary={boundary}"
                );
                assert_eq!(inputs.samples.get(), 1);
            }
        }
    }
}

#[test]
fn flat_positive_faces_beyond_unit_bounds_keep_the_own_cell_light() {
    for face in [Face::PositiveX, Face::PositiveY, Face::PositiveZ] {
        let (normal, _, _) = face_basis(face);
        let axis = normal.iter().position(|&value| value != 0).unwrap();
        let mut positions = [[128; 3]; 4];
        for position in &mut positions {
            position[axis] = 288;
        }
        let inputs = FlatInputs {
            samples: Cell::new(0),
        };
        let actual = bake_quad_with(&inputs, [0; 3], face, positions, true, false);
        assert_eq!(actual.samples(), [pack_sample(3, 7, 0); 4]);
        assert_eq!(inputs.samples.get(), 1);
    }
}
