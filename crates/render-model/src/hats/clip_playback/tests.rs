use super::*;
use glam::Quat;
use serde_json::json;

/// A channel whose shortest final turn ends one full turn below its authored endpoint.
fn wrapped_clip(looping: bool) -> Definition {
    serde_json::from_value(json!({
        "loop": looping,
        "animation_length": 2.0,
        "bones": {"head": {"rotation": {
            "0": [0, 170, 0],
            "1": [0, -170, 0],
            "2": [0, 170, 0]
        }}}
    }))
    .unwrap()
}

/// Compares rendered yaw orientations on either side of a playback transition.
fn assert_continuous(clips: &Clips, boundary: f32) {
    let before = clips.sample(0, boundary - 0.0001).0.y.to_radians();
    let after = clips.sample(0, boundary + 0.0001).0.y.to_radians();
    assert!(
        Quat::from_rotation_y(before)
            .dot(Quat::from_rotation_y(after))
            .abs()
            > 0.99999
    );
}

#[test]
fn wrapped_rotations_admit_and_play_continuously_across_loop_seams() {
    let clips = Clips::build(
        BTreeMap::from([("spin".into(), wrapped_clip(true))]),
        None,
        &["head"],
    )
    .expect("equivalent wrapped endpoints admit");
    assert_eq!(clips.sample(0, 0.5).0.y, 180.0);
    assert_eq!(clips.sample(0, 1.5).0.y, -180.0);
    assert_continuous(&clips, 2.0);
}

#[test]
fn wrapped_rotations_admit_across_intro_sequence_and_cycle_seams() {
    let clips = Clips::build(
        BTreeMap::from([
            ("intro".into(), wrapped_clip(false)),
            ("first".into(), wrapped_clip(false)),
            ("second".into(), wrapped_clip(false)),
        ]),
        Some(Sequence {
            intro: Some("intro".into()),
            cycle: vec![
                Step {
                    animation: "first".into(),
                    delay: 0.25,
                },
                Step {
                    animation: "second".into(),
                    delay: 0.25,
                },
            ],
        }),
        &["head"],
    )
    .expect("equivalent wrapped sequence endpoints admit");
    for boundary in [2.0, 4.25, 6.5] {
        assert_continuous(&clips, boundary);
    }
}
