use super::*;
use serde_json::{Value, json};

/// Reads motion timing from the shipped asset so evidence follows the authored sequence.
fn definition() -> Value {
    serde_json::from_slice(include_bytes!("shoulder_parrot.animation.json")).unwrap()
}

/// Evaluates the full parrot hierarchy in the standard model frame.
fn poses(model: &HatRig, seconds: f32) -> Arc<[RenderBoneTransform]> {
    let root = model.geometry.bone_pivots[model.head];
    model.pose(
        RenderBoneTransform {
            rotation: Quat::IDENTITY.to_array(),
            translation_scale: [root[0], root[1], root[2], 1.0],
            axis_scale: crate::UNIT_AXIS_SCALE,
        },
        Motion {
            seconds,
            ..Default::default()
        },
    )
}

#[test]
fn parrot_arrives_once_rests_and_alternates_shoulders_without_teleporting() {
    let item = fixture::shoulder_parrot();
    let model = rig(item);
    let playback = model.playback.as_ref().unwrap();
    let asset = definition();
    let intro =
        asset["animations"][asset["playback"]["intro"].as_str().unwrap()]["animation_length"]
            .as_f64()
            .unwrap() as f32;
    let body = 1;
    let first = poses(model, intro);
    let start = poses(model, 0.0);
    assert!(start[body].translation_scale[0] < first[body].translation_scale[0] - 0.5);
    let mut time = intro;
    let mut previous = first;
    for (index, step) in asset["playback"]["cycle"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let delay = step["delay"].as_f64().unwrap() as f32;
        let held = poses(model, time + delay * 0.5);
        for (a, b) in previous.iter().zip(held.iter()) {
            assert!(
                Vec3::from_slice(&a.translation_scale[..3])
                    .distance(Vec3::from_slice(&b.translation_scale[..3]))
                    < 1e-5,
                "the perch holds still between flights"
            );
            assert!(
                Quat::from_array(a.rotation)
                    .dot(Quat::from_array(b.rotation))
                    .abs()
                    > 0.999999
            );
        }
        time += delay;
        let length = asset["animations"][step["animation"].as_str().unwrap()]["animation_length"]
            .as_f64()
            .unwrap() as f32;
        let middle = poses(model, time + length * 0.5);
        assert!(
            middle[body].translation_scale[1] > 2.1,
            "the bird crosses above the head"
        );
        for boundary in [time, time + length] {
            let before = poses(model, boundary - 0.0001);
            let after = poses(model, boundary + 0.0001);
            for (a, b) in before.iter().zip(after.iter()) {
                assert!(
                    Vec3::from_slice(&a.translation_scale[..3])
                        .distance(Vec3::from_slice(&b.translation_scale[..3]))
                        < 0.002
                );
                assert!(
                    Quat::from_array(a.rotation)
                        .dot(Quat::from_array(b.rotation))
                        .abs()
                        > 0.999
                );
            }
        }
        time += length;
        let landing = poses(model, time);
        assert!((landing[body].translation_scale[1] - 25.5 / 16.0).abs() < 1e-5);
        assert_eq!(landing[body].translation_scale[0] > 0.0, index % 2 == 0);
        previous = landing;
    }
    let mut motion = Motion::default();
    motion.advance(item, time + 0.5);
    assert!(
        (motion.seconds - (intro + 0.5)).abs() < 1e-4,
        "wrapping excludes the arrival"
    );
    assert_eq!(*poses(model, motion.seconds), *poses(model, intro + 0.5));
    for invalid in [f32::NAN, f32::INFINITY, -1.0] {
        motion.advance(item, invalid);
    }
    assert!(motion.seconds.is_finite());
    motion.advance(item, f32::MAX);
    assert!(motion.seconds >= intro && motion.seconds < playback.length);
}

#[test]
fn flight_translation_inherits_player_rotation_and_scale() {
    let model = rig(fixture::shoulder_parrot());
    let seconds = 0.7;
    let base = poses(model, seconds);
    let rotation = Quat::from_rotation_y(0.8);
    let offset = Vec3::new(2.0, 3.0, 4.0);
    let root = Vec3::from_array(model.geometry.bone_pivots[model.head]);
    let scale = Vec3::new(0.7, 1.3, 0.9);
    let actual = model.pose(
        RenderBoneTransform {
            rotation: rotation.to_array(),
            translation_scale: [offset.x, offset.y, offset.z, 1.0],
            axis_scale: [scale.x, scale.y, scale.z, 0.0],
        },
        Motion {
            seconds,
            ..Default::default()
        },
    );
    for (a, b) in actual.iter().zip(base.iter()) {
        // Root scale applies to the authored joint offsets; parent rotations then orient them.
        assert!(a.translation_scale.iter().all(|v| v.is_finite()));
        assert!(
            Quat::from_array(a.rotation)
                .dot(rotation * Quat::from_array(b.rotation))
                .abs()
                > 0.99999
        );
    }
    let expected =
        offset + rotation * ((Vec3::from_slice(&base[1].translation_scale[..3]) - root) * scale);
    assert!(Vec3::from_slice(&actual[1].translation_scale[..3]).distance(expected) < 1e-5);
}

#[test]
fn parrot_keeps_flapping_until_touchdown_and_clears_the_head() {
    let model = rig(fixture::shoulder_parrot());
    let asset = definition();
    let mut start = 0.0;
    let mut clips = vec![(asset["playback"]["intro"].as_str().unwrap(), 0.0)];
    clips.extend(
        asset["playback"]["cycle"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| {
                (
                    s["animation"].as_str().unwrap(),
                    s["delay"].as_f64().unwrap() as f32,
                )
            }),
    );
    for (name, delay) in clips {
        start += delay;
        let length = asset["animations"][name]["animation_length"]
            .as_f64()
            .unwrap() as f32;
        let late = poses(model, start + length * 0.94);
        let landed = poses(model, start + length);
        assert!(
            late.iter()
                .zip(landed.iter())
                .skip(3)
                .take(2)
                .any(|(a, b)| Quat::from_array(a.rotation)
                    .dot(Quat::from_array(b.rotation))
                    .abs()
                    < 0.98),
            "wingbeats continue during the final approach"
        );
        for frame in 0..=120 {
            let pose = poses(model, start + length * frame as f32 / 120.0);
            for vertex in model.geometry.vertices.iter() {
                let bone = vertex.bone_index as usize;
                let p = Vec3::from_slice(&pose[bone].translation_scale[..3])
                    + Quat::from_array(pose[bone].rotation)
                        * (Vec3::from_array(vertex.position)
                            - Vec3::from_array(model.geometry.bone_pivots[bone]));
                assert!(p.is_finite());
                assert!(
                    !(p.x.abs() < 0.25 && p.z.abs() < 0.25 && p.y > 1.5 && p.y < 2.0),
                    "flight vertex intersects head at {name} {frame}: {p:?}"
                );
            }
        }
        start += length;
    }
}

#[test]
fn malformed_flight_channels_and_sequences_fail_before_admission() {
    let original = definition();
    let mut invalid = Vec::new();
    for (pointer, replacement) in [
        ("/playback/intro", json!("missing")),
        ("/playback/cycle/0/delay", json!(-1)),
        ("/playback/cycle/0/animation", json!("missing")),
        ("/playback/cycle", json!([])),
        (
            "/animations/parrot.arrival/bones/parrot_body/position/0.0",
            json!([65, 0, 0]),
        ),
        (
            "/animations/parrot.arrival/bones/parrot_body/position/0.00",
            json!([1, 0, 0]),
        ),
        (
            "/animations/parrot.to_left/bones/parrot_body/position/2.8",
            json!([0, 0, 0]),
        ),
    ] {
        let mut value = original.clone();
        // Inserting object keys exercises absent keys as well as existing paths.
        let (parent, key) = pointer.rsplit_once('/').unwrap();
        if let Some(target) = value.pointer_mut(pointer) {
            *target = replacement;
        } else {
            value
                .pointer_mut(parent)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert(key.into(), replacement);
        }
        invalid.push((pointer, value));
    }
    for (path, value) in invalid {
        assert!(
            model::build(
                fixture::shoulder_parrot().descriptor(),
                include_str!("shoulder_parrot.geo.json"),
                include_bytes!("shoulder_parrot.png"),
                &serde_json::to_vec(&value).unwrap()
            )
            .is_none(),
            "must reject {path}"
        );
    }
}
