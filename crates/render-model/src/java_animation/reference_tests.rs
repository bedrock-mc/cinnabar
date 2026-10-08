use super::*;
use serde_json::Value;

fn number(value: &Value) -> f32 {
    value.as_f64().expect("numeric fixture") as f32
}

fn vector(value: &Value) -> Vec3 {
    Vec3::from_array(std::array::from_fn(|index| number(&value[index])))
}

fn near(actual: Vec3, expected: Vec3, state: &str, part: &str) {
    assert!(
        (actual - expected).abs().max_element() < 2e-6,
        "{state} {part}: {actual} != {expected}"
    );
}

fn matrix(value: &Value) -> Mat4 {
    Mat4::from_cols_array(&std::array::from_fn(|index| number(&value[index])))
}

fn near_matrix(actual: Mat4, expected: Mat4, state: &str) {
    assert!(
        actual.abs_diff_eq(expected, 2e-6),
        "{state}: {actual} != {expected}"
    );
}

#[test]
fn biped_matches_fixed_native_java_states() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/biped.json")).unwrap();
    for state in fixture["states"].as_array().unwrap() {
        let input = &state["input"];
        let pose = java_biped(&JavaBipedInput {
            limb_swing: number(&input["phase"]),
            limb_amount: number(&input["amount"]),
            age: number(&input["age"]),
            head_yaw: number(&input["yaw"]),
            head_pitch: number(&input["pitch"]),
            swing: number(&input["swing"]),
            sneaking: input["sneaking"].as_bool().unwrap(),
            riding: input["riding"].as_bool().unwrap(),
            held_right: input["held"].as_u64().unwrap() as u8,
            aimed_bow: input["bow"].as_bool().unwrap(),
        });
        for (name, part) in [
            ("head", pose.head),
            ("body", pose.body),
            ("right_arm", pose.right_arm),
            ("left_arm", pose.left_arm),
            ("right_leg", pose.right_leg),
            ("left_leg", pose.left_leg),
        ] {
            let expected = &state["parts"][name];
            let state_name = state["name"].as_str().unwrap();
            near(part.point, vector(&expected["point"]), state_name, name);
            near(part.angles, vector(&expected["angles"]), state_name, name);
        }
    }
}

#[test]
fn cape_matches_fixed_native_java_gl_states() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/cape.json")).unwrap();
    let pivot = Vec3::new(0.0, 24.0, 3.0) / 16.0;
    let rig = rig_from_java_model();
    let rest = Mat4::from_translation(Vec3::Z * 0.125) * Mat4::from_rotation_y(PI);
    for state in fixture["states"].as_array().unwrap() {
        let input = JavaCapeInput {
            chase: vector(&state["delta"]),
            body_yaw: number(&state["yaw"]),
            walked: number(&state["walk"]),
            bob: number(&state["bob"]),
            sneaking: state["sneaking"].as_bool().unwrap(),
        };
        let (tilt, side) = java_cape_angles(&input);
        let state_name = state["name"].as_str().unwrap();
        assert!(
            (tilt - number(&state["tilt_side"][0])).abs() < 2e-5,
            "{state_name}: cape tilt {tilt}"
        );
        assert!(
            (side - number(&state["tilt_side"][1])).abs() < 2e-5,
            "{state_name}: cape side {side}"
        );
        let matrix = Mat4::from_cols_array(&std::array::from_fn(|index| {
            number(&state["matrix"][index])
        }));
        let (rotation, translation) = java_cape_bone(&input, pivot);
        for x in [-5.0, 5.0] {
            for y in [0.0, 16.0] {
                for z in [-1.0, 0.0] {
                    let corner = Vec3::new(x, y, z) / 16.0;
                    let bind = rig.transform_point3(rest.transform_point3(corner));
                    let ours = rotation * (bind - pivot) + translation;
                    let expected = rig.transform_point3(matrix.transform_point3(corner));
                    near(ours, expected, state_name, "cape corner");
                }
            }
        }
    }
}

#[test]
fn first_person_item_matches_fixed_native_java_gl_states() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/hand.json")).unwrap();
    for state in fixture["states"].as_array().unwrap() {
        let duration = number(&state["duration"]);
        let remaining = number(&state["remaining"]) - number(&state["fraction"]) + 1.0;
        let using = match state["action"].as_str().unwrap() {
            "none" => None,
            "eat" | "drink" => Some(JavaUse::Consume {
                remaining,
                duration,
            }),
            "block" => Some(JavaUse::Block),
            "bow" => Some(JavaUse::Bow {
                pull: duration - remaining,
            }),
            action => panic!("unknown use {action}"),
        };
        let ours = first_person_item(
            JavaHand {
                swing: number(&state["swing"]),
                equip: number(&state["equip"]),
                using,
            },
            JavaItemMesh::Sprite,
            state["rotate"].as_bool().unwrap(),
        );
        // Compare in the unit sprite slab's draw frame, before our -X mesh translation.
        let ours = ours * Mat4::from_translation(-Vec3::X);
        near_matrix(
            ours,
            matrix(&state["matrix"]),
            state["name"].as_str().unwrap(),
        );
    }
}

#[test]
fn first_person_arm_matches_fixed_native_java_gl_states() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/arm.json")).unwrap();
    for state in fixture["states"].as_array().unwrap() {
        let ours = first_person_arm(number(&state["swing"]), number(&state["equip"]))
            * rig_from_java_model();
        near_matrix(
            ours,
            matrix(&state["matrix"]),
            state["name"].as_str().unwrap(),
        );
    }
}
