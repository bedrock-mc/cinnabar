use std::collections::HashMap;

use super::*;
use actor_animation::{
    ActorAnimationStore, ActorTickContext, MAX_MOLANG_OPS_PER_RENDER_FRAME,
    test_support::{quat_from_euler, total_scale},
};

#[test]
fn installed_creeper_samples_pack_swelling_and_flash_between_ticks() {
    assert_installed_creeper_samples(false);
}

#[test]
fn installed_powered_creeper_samples_pack_swelling_and_motion_between_ticks() {
    assert_installed_creeper_samples(true);
}

/// Checks fractional swelling and head motion on every installed creeper layer.
fn assert_installed_creeper_samples(powered: bool) {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/assets/compiled");
    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!(
                "skipping creeper animation fixture: {} is absent",
                root.display()
            );
            return;
        }
        Err(error) => panic!("read {}: {error}", root.display()),
    };
    let Some(path) = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.extension()
                .is_some_and(|extension| extension == "mcbeent")
        })
    else {
        eprintln!(
            "skipping creeper animation fixture: no entity carrier in {}",
            root.display()
        );
        return;
    };
    let assets = Arc::new(RuntimeEntityAssets::decode(&std::fs::read(path).unwrap()).unwrap());
    let mut store =
        crate::actor_store::ActorStore::new_with_entity_assets(1, 0, Arc::clone(&assets));
    store.apply(
        1,
        1,
        protocol::ActorEvent::Spawn(protocol::ActorSpawnEvent {
            dimension: 0,
            unique_id: 1,
            runtime_id: 1,
            kind: ActorKind::Entity {
                identifier: "minecraft:creeper".into(),
            },
            position: [0.0; 3],
            velocity: [0.0; 3],
            pitch: 0.0,
            yaw: 0.0,
            head_yaw: 0.0,
            body_yaw: 0.0,
            held_item: Default::default(),
            metadata: Arc::from([protocol::ActorMetadata {
                key: 0,
                value: ActorMetadataValue::Flags((1 << 10) | if powered { 1 << 9 } else { 0 }),
            }]),
            attributes: Arc::from([]),
            properties: Arc::from([]),
            links: Arc::from([]),
        }),
    );
    store.advance_interpolation_ticks(2);
    let rig = store.actor_rig(1).unwrap();
    let body = rig
        .bone_names
        .iter()
        .position(|name| name.as_ref() == "body")
        .unwrap();
    let exact = store.render_frame(0.0).layers(1).unwrap();
    let drawn = if exact[0].pose.is_empty() {
        &rig.previous[body]
    } else {
        &exact[0].previous_pose[body]
    };
    assert_eq!(
        drawn.axis_scale, rig.current[body].axis_scale,
        "zero fraction must sample the same swelling as the completed tick"
    );
    let tick = rig.completed_tick;
    let early = store.render_frame(0.25).layers(1).unwrap().into_owned();
    let late = store.render_frame(0.75).layers(1).unwrap().into_owned();
    assert_ne!(
        early[0].pose, late[0].pose,
        "pack swell must sample the frame fraction"
    );
    assert_eq!(early[0].overlay[3], 0.0);
    assert_eq!(late[0].overlay, [1.0, 1.0, 1.0, 0.5]);
    assert_eq!(store.actor_rig(1).unwrap().completed_tick, tick);
    assert_eq!(early, store.render_frame(0.25).layers(1).unwrap().as_ref());
    store.apply(
        1,
        2,
        protocol::ActorEvent::Move(protocol::ActorMoveEvent {
            dimension: 0,
            runtime_id: 1,
            position: [Some(1.0), None, None],
            position_origin: protocol::ActorPositionOrigin::Feet,
            pitch: Some(30.0),
            yaw: None,
            head_yaw: None,
            on_ground: Some(true),
            teleported: false,
            player_mode: None,
            source_tick: None,
            interpolation: Default::default(),
        }),
    );
    store.advance_interpolation_ticks(1);
    let rig = store.actor_rig(1).unwrap();
    let head = rig
        .bone_names
        .iter()
        .position(|name| name.as_ref() == "head")
        .unwrap();
    assert_ne!(rig.previous[head].rotation, rig.current[head].rotation);
    let actor = store.get(1).unwrap();
    let previous_pitch = actor.previous_pose.pitch;
    let pitch = actor.pitch;
    for alpha in [0.0, 0.25, 0.75, 1.0] {
        let expected = quat_from_euler([
            -(previous_pitch + (pitch - previous_pitch) * alpha),
            0.0,
            0.0,
        ]);
        let layers = store.render_frame(alpha).layers(1).unwrap();
        if powered {
            assert!(layers.len() > 1);
        }
        for (index, layer) in layers.iter().enumerate() {
            let drawn = interpolated_pitch(
                [
                    layer.previous_pose[head].rotation,
                    layer.pose[head].rotation,
                ],
                alpha,
            );
            let expected = interpolated_pitch([expected; 2], alpha);
            assert!(
                (drawn - expected).abs() < 1e-4,
                "layer {index} head pitch at {alpha}: {drawn}, expected {expected}"
            );
        }
    }

    let mut actor = store.get(1).unwrap().clone();
    let mut animation = ActorAnimationStore::with_assets(Arc::clone(&assets));
    animation.insert(1, 0, &actor);
    animation.advance_tick(
        &HashMap::from([(1, actor.clone())]),
        None,
        None,
        true,
        true,
        |_| ActorTickContext::default(),
    );
    actor.pitch = -30.0;
    animation.advance_tick(
        &HashMap::from([(1, actor.clone())]),
        None,
        None,
        true,
        true,
        |_| ActorTickContext::default(),
    );
    assert_ne!(
        animation.get(1).unwrap().previous[head].rotation,
        animation.get(1).unwrap().current[head].rotation
    );
    animation.set_world_budget(0);
    animation.advance_tick(
        &HashMap::from([(1, actor.clone())]),
        None,
        None,
        true,
        true,
        |_| ActorTickContext::default(),
    );
    let held = animation.get(1).unwrap();
    assert_eq!(held.previous[head].rotation, held.current[head].rotation);
    for alpha in [0.0, 0.25, 0.75] {
        let mut remaining = MAX_MOLANG_OPS_PER_RENDER_FRAME;
        let layers = animation
            .render_layers(&actor, alpha, [0.0; 2], [0.0; 3], &mut remaining, false)
            .unwrap();
        for layer in layers.render.iter().filter(|layer| !layer.pose.is_empty()) {
            assert_eq!(
                layer.previous_pose[head].rotation, layer.pose[head].rotation,
                "frozen motion must stay held while swelling is sampled"
            );
        }
    }
    let mut steps = 0;
    while store.get(1).unwrap().creeper_swell_changes() {
        store.advance_interpolation_ticks(1);
        steps += 1;
        assert!(steps < 100, "swelling must reach a stable cap");
    }
    let capped = store.actor_rig(1).unwrap();
    let amount = store.get(1).unwrap().creeper_swell_amount(0.0);
    let wobble = (amount * 5730.0).to_radians().sin() * amount * 0.01 + 1.0;
    let growth = amount.clamp(0.0, 1.0).powi(4);
    let expected_body_scale = [
        (growth * 0.4 + 1.0) * wobble,
        (growth * 0.1 + 1.0) / wobble,
        (growth * 0.4 + 1.0) * wobble,
    ];
    for (actual, expected) in total_scale(&capped.current[body])
        .into_iter()
        .zip(expected_body_scale)
    {
        assert!(
            (actual - expected).abs() < 1e-5,
            "the authored body swell must be applied once: {actual} != {expected}"
        );
    }
    for alpha in [0.0, 0.25, 0.75] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        let previous = if layers[0].previous_pose.is_empty() {
            &capped.previous[body]
        } else {
            &layers[0].previous_pose[body]
        };
        assert_eq!(
            total_scale(previous),
            total_scale(&capped.current[body]),
            "the first steady fuse tick must hold its capped scale"
        );
        if powered {
            assert_eq!(
                total_scale(&layers[1].previous_pose[body]),
                total_scale(&capped.current[body])
            );
        }
    }
    store.apply(
        1,
        3,
        protocol::ActorEvent::Metadata(protocol::ActorMetadataUpdateEvent {
            dimension: 0,
            runtime_id: 1,
            tick: 0,
            metadata: Arc::from([protocol::ActorMetadata {
                key: 0,
                value: ActorMetadataValue::Flags(if powered { 1 << 9 } else { 0 }),
            }]),
            properties: Arc::from([]),
        }),
    );
    store.advance_interpolation_ticks(1);
    steps = 0;
    while store.get(1).unwrap().creeper_swell_changes() {
        store.advance_interpolation_ticks(1);
        steps += 1;
        assert!(steps < 100, "defusing must reach a stable rest pose");
    }
    let defused = store.actor_rig(1).unwrap();
    for alpha in [0.0, 0.25, 0.75] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        let previous = if layers[0].previous_pose.is_empty() {
            &defused.previous[body]
        } else {
            &layers[0].previous_pose[body]
        };
        assert_eq!(
            total_scale(previous),
            total_scale(&defused.rest[body]),
            "the first steady defused tick must hold its rest scale"
        );
        if powered {
            assert_eq!(
                total_scale(&layers[1].previous_pose[body]),
                total_scale(&defused.rest[body])
            );
        }
    }
    store.advance_interpolation_ticks(1);
    // A fresh stationary rig has no decaying walk speed that still needs frame sampling.
    let actor = store.get(1).unwrap().clone();
    let mut resting = ActorAnimationStore::with_assets(assets);
    resting.insert(1, 0, &actor);
    let actors = HashMap::from([(1, actor)]);
    for _ in 0..2 {
        resting.advance_tick(&actors, None, None, true, true, |_| {
            ActorTickContext::default()
        });
    }
    let completed = resting.get(1).unwrap();
    for alpha in [0.25, 0.75] {
        let mut remaining = MAX_MOLANG_OPS_PER_RENDER_FRAME;
        let layers = resting
            .render_layers(
                &actors[&1],
                alpha,
                [0.0; 2],
                [0.0; 3],
                &mut remaining,
                false,
            )
            .unwrap()
            .render;
        if powered {
            assert!(Arc::ptr_eq(&completed.render[1].pose, &layers[1].pose));
            assert!(Arc::ptr_eq(
                &completed.render[1].previous_pose,
                &layers[1].previous_pose
            ));
        }
        for layer in layers.iter().take(if powered { 2 } else { 1 }) {
            for (pose, fallback) in [
                (layer.previous_pose.as_ref(), completed.previous),
                (layer.pose.as_ref(), completed.current),
            ] {
                let body_pose = if pose.is_empty() {
                    &fallback[body]
                } else {
                    &pose[body]
                };
                assert_eq!(
                    total_scale(body_pose),
                    total_scale(&completed.rest[body]),
                    "a steady defused creeper must retain its rest scale"
                );
            }
        }
        assert!(
            layers[0].pose.is_empty() && layers[0].previous_pose.is_empty(),
            "unchanged swelling must reuse tick-owned poses"
        );
    }

    store.apply(
        1,
        4,
        protocol::ActorEvent::Metadata(protocol::ActorMetadataUpdateEvent {
            dimension: 0,
            runtime_id: 1,
            tick: 0,
            metadata: Arc::from([protocol::ActorMetadata {
                key: 0,
                value: ActorMetadataValue::Flags((1 << 10) | if powered { 1 << 9 } else { 0 }),
            }]),
            properties: Arc::from([]),
        }),
    );
    store.advance_interpolation_ticks(10);
    store.apply(
        1,
        5,
        protocol::ActorEvent::Status(protocol::ActorStatusEvent {
            runtime_id: 1,
            kind: protocol::ActorStatusKind::Death,
            data: 0,
        }),
    );
    let dying = store.actor_rig(1).unwrap();
    for alpha in [0.0, 0.25, 0.75] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        let previous = if layers[0].previous_pose.is_empty() {
            &dying.previous[body]
        } else {
            &layers[0].previous_pose[body]
        };
        assert_eq!(
            total_scale(previous),
            total_scale(&dying.rest[body]),
            "death clears swelling before another animation tick"
        );
        if powered {
            assert_eq!(
                total_scale(&layers[1].previous_pose[body]),
                total_scale(&dying.rest[body])
            );
        }
    }
}

/// Checks the pitch of the blended transformed vertices between tick endpoints.
#[test]
fn displayed_pitch_follows_interpolated_vertices() {
    let radians = 30.0_f32.to_radians();
    let half = radians * 0.5;
    let rotations = [[0.0, 0.0, 0.0, 1.0], [half.sin(), 0.0, 0.0, half.cos()]];
    for alpha in [0.25, 0.75] {
        let expected = (alpha * radians.sin()).atan2(1.0 - alpha + alpha * radians.cos());
        assert!(
            (interpolated_pitch(rotations, alpha) - expected).abs() < 1.0e-6,
            "displayed pitch follows the blended transformed up vector"
        );
    }
}

/// Measures this fixture's pure X-axis pitch after blending transformed up vectors.
fn interpolated_pitch(rotations: [[f32; 4]; 2], alpha: f32) -> f32 {
    let up = rotations.map(|[x, y, z, w]| {
        assert!(
            y.abs() < 1e-6 && z.abs() < 1e-6,
            "fixture must rotate around X"
        );
        [2.0 * x * w, 1.0 - 2.0 * x * x]
    });
    let sine = up[0][0] + (up[1][0] - up[0][0]) * alpha;
    let cosine = up[0][1] + (up[1][1] - up[0][1]) * alpha;
    sine.atan2(cosine)
}
