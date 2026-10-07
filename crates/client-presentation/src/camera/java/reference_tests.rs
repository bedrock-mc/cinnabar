use super::*;
use serde_json::Value;

fn number(value: &Value) -> f32 {
    value.as_f64().expect("numeric fixture") as f32
}

fn compare(actual: Mat4, state: &Value) {
    let expected = Mat4::from_cols_array(&std::array::from_fn(|index| {
        number(&state["matrix"][index])
    }));
    assert!(
        actual.abs_diff_eq(expected, 2e-6),
        "{}: {actual} != {expected}",
        state["input"]
    );
}

#[test]
fn view_bob_matches_fixed_native_java_gl_states() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures.json")).unwrap();
    for state in fixture["bob"].as_array().unwrap() {
        let input = &state["input"];
        let camera = JavaCameraState {
            walked: [number(&input[0]), number(&input[1])],
            bob: [number(&input[2]), number(&input[3])],
            tilt: [number(&input[4]), number(&input[5])],
            ..Default::default()
        };
        compare(camera.bob(number(&input[6])).matrix(), state);
    }
}

#[test]
fn hurt_roll_matches_fixed_native_java_gl_states() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures.json")).unwrap();
    for state in fixture["hurt"].as_array().unwrap() {
        let input = &state["input"];
        let progress = (number(&input[0]) - number(&input[2])) / number(&input[1]);
        compare(java_hurt_roll(progress), state);
    }
}

#[test]
fn hand_sway_matches_fixed_native_java_gl_states() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures.json")).unwrap();
    for state in fixture["sway"].as_array().unwrap() {
        let input = &state["input"];
        let camera = JavaCameraState {
            arm_pitch: [number(&input[0]), number(&input[1])],
            arm_yaw: [number(&input[2]), number(&input[3])],
            ..Default::default()
        };
        let (pitch, yaw) = camera.sway(number(&input[6]), [number(&input[4]), number(&input[5])]);
        compare(
            Mat4::from_rotation_x(pitch) * Mat4::from_rotation_y(yaw),
            state,
        );
    }
}
