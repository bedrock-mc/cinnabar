#[path = "../src/action.rs"]
#[allow(dead_code)]
mod action;
#[path = "../src/geometry.rs"]
#[allow(dead_code)]
mod geometry;
#[path = "../src/settings.rs"]
#[allow(dead_code)]
mod settings;

use action::{PointerPhase, UiAction, UiLimits};
use geometry::{DpiScale, SafeArea, UiPoint, UiRect, UiScale};
use settings::{CURRENT_SETTINGS_SCHEMA, UserSettings};

#[test]
fn scale_and_geometry_reject_non_finite_or_inverted_values() {
    assert!(UiScale::new(f32::NAN).is_err());
    assert!(UiScale::new(0.49).is_err());
    assert_eq!(UiScale::new(2.0).unwrap().get(), 2.0);
    assert_eq!(
        DpiScale::new(1.5).unwrap().physical_to_logical(150.0),
        100.0
    );
    assert!(
        UiRect::new(
            UiPoint::new(5.0, 0.0).unwrap(),
            UiPoint::new(4.0, 1.0).unwrap()
        )
        .is_err()
    );
    assert!(UiPoint::new(f32::INFINITY, 0.0).is_err());
    assert!(SafeArea::new(0.0, -1.0, 0.0, 0.0).is_err());
}

#[test]
fn actions_are_device_neutral_and_limits_are_fixed() {
    assert_eq!(UiAction::Accept, UiAction::Accept);
    let point = UiPoint::new(10.0, 20.0).unwrap();
    assert_eq!(
        UiAction::PointerPrimary {
            position: point,
            phase: PointerPhase::Pressed
        },
        UiAction::PointerPrimary {
            position: point,
            phase: PointerPhase::Pressed
        }
    );
    assert_eq!(UiLimits::MAX_NODES, 16_384);
    assert_eq!(UiLimits::MAX_TEXT_BYTES, 16_384);
    assert_eq!(UiLimits::MAX_FOCUSABLE, 4_096);
    assert_eq!(UiLimits::MAX_CLIP_DEPTH, 32);
    assert_eq!(SafeArea::ZERO.left(), 0.0);
}

#[test]
fn physical_input_maps_to_the_same_logical_target_at_supported_dpi_scales() {
    let target = UiRect::new(
        UiPoint::new(40.0, 20.0).unwrap(),
        UiPoint::new(80.0, 60.0).unwrap(),
    )
    .unwrap();

    for scale in [1.0, 1.25, 1.5, 2.0, 3.0] {
        let dpi = DpiScale::new(scale).unwrap();
        let action = UiAction::pointer_primary_from_physical(
            [60.0 * scale, 40.0 * scale],
            PointerPhase::Pressed,
            dpi,
        )
        .unwrap();
        let UiAction::PointerPrimary { position, .. } = action else {
            unreachable!();
        };
        assert_eq!(position, UiPoint::new(60.0, 40.0).unwrap());
        assert!(target.contains(position));

        let clip = UiRect::from_physical(
            [40.0 * scale, 20.0 * scale],
            [80.0 * scale, 60.0 * scale],
            dpi,
        )
        .unwrap();
        assert_eq!(clip, target);
    }
}

#[test]
fn physical_action_constructors_reject_non_finite_coordinates() {
    let dpi = DpiScale::new(1.0).unwrap();
    assert!(UiAction::pointer_move_from_physical([f32::NAN, 0.0], dpi).is_err());
    assert!(UiAction::scroll_from_physical([0.0, f32::INFINITY], dpi).is_err());
    assert!(DpiScale::new(0.0).is_err());
    assert!(DpiScale::new(-1.0).is_err());
}

#[test]
fn settings_interface_has_versioned_typed_sections() {
    let settings = UserSettings::default();
    assert_eq!(settings.schema_version, CURRENT_SETTINGS_SCHEMA);
    assert!(settings.video.horizontal_fov_degrees.is_finite());
    assert!(settings.controls.bindings().len() <= 128);
}
