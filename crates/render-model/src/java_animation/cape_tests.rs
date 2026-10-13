use super::*;

/// Checks the cape's centre line, independently of its thickness or bind-pivot placement.
fn assert_below_shoulders(input: JavaCapeInput) {
    let (tilt, side) = java_cape_angles(&input);
    assert!(tilt <= CAPE_MAX_SWING_DEGREES);
    assert!(side.abs() <= CAPE_MAX_SWING_DEGREES);
    let (rotation, _) = java_cape_bone(&input, Vec3::new(0.0, 1.5, 0.1875));
    assert!(
        (rotation * Vec3::NEG_Y).y <= 1e-6,
        "cape points above its shoulder: {input:?}"
    );
}

#[test]
fn vertical_flight_keeps_the_lift_clamps() {
    for (height, expected) in [(-30.0, 0.0), (-0.2, 4.0), (0.2, 8.0), (30.0, 38.0)] {
        let input = JavaCapeInput {
            chase: Vec3::Y * height,
            ..Default::default()
        };
        assert_eq!(java_cape_angles(&input), (expected, 0.0));
        assert_below_shoulders(input);
    }
}

#[test]
fn fast_forward_flight_bounds_the_combined_tilt() {
    for height in [-30.0, 0.0, 30.0] {
        for sneaking in [false, true] {
            let input = JavaCapeInput {
                chase: Vec3::new(0.0, height, -6.0),
                bob: 0.1,
                walked: 0.25,
                sneaking,
                ..Default::default()
            };
            assert_eq!(java_cape_angles(&input).0, CAPE_MAX_SWING_DEGREES);
            assert_below_shoulders(input);
        }
    }
}

#[test]
fn fast_sideways_flight_cannot_invert_the_cape() {
    for sideways in [-6.0, 6.0] {
        for back in [0.0, -6.0] {
            let input = JavaCapeInput {
                chase: Vec3::new(sideways, 0.0, back),
                ..Default::default()
            };
            assert_eq!(java_cape_angles(&input).1.abs(), CAPE_MAX_SWING_DEGREES);
            assert_below_shoulders(input);
        }
    }
}
