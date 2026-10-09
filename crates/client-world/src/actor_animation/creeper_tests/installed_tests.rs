use super::*;

#[test]
fn installed_creeper_samples_pack_swelling_and_flash_between_ticks() {
    assert_installed_creeper_samples(false);
}

#[test]
fn installed_powered_creeper_samples_pack_swelling_and_motion_between_ticks() {
    assert_installed_creeper_samples(true);
}

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
    let expected = [rig.previous[head].rotation, rig.current[head].rotation];
    for alpha in [0.25, 0.75] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        if powered {
            assert!(layers.len() > 1);
            assert_eq!(
                [
                    layers[1].previous_pose[head].rotation,
                    layers[1].pose[head].rotation
                ],
                expected,
                "powered layer must preserve motion"
            );
        }
        assert_eq!(
            [
                layers[0].previous_pose[head].rotation,
                layers[0].pose[head].rotation
            ],
            expected,
            "swelling must retain tick interpolation for head motion"
        );
    }
    let mut actor = store.get(1).unwrap().clone();
    let mut animation = ActorAnimationStore::with_assets(assets);
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
    animation.schedule.world_budget = 0;
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
    for (actual, expected) in pose::total_scale(&capped.current[body])
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
            pose::total_scale(previous),
            pose::total_scale(&capped.current[body]),
            "the first steady fuse tick must hold its capped scale"
        );
        if powered {
            assert_eq!(
                pose::total_scale(&layers[1].previous_pose[body]),
                pose::total_scale(&capped.current[body])
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
            pose::total_scale(previous),
            pose::total_scale(&defused.rest[body]),
            "the first steady defused tick must hold its rest scale"
        );
        if powered {
            assert_eq!(
                pose::total_scale(&layers[1].previous_pose[body]),
                pose::total_scale(&defused.rest[body])
            );
        }
    }
    store.advance_interpolation_ticks(1);
    let completed = store.actor_rig(1).unwrap().render;
    for alpha in [0.25, 0.75] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        if powered {
            assert!(Arc::ptr_eq(&completed[1].pose, &layers[1].pose));
            assert!(Arc::ptr_eq(
                &completed[1].previous_pose,
                &layers[1].previous_pose
            ));
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
            pose::total_scale(previous),
            pose::total_scale(&dying.rest[body]),
            "death clears swelling before another animation tick"
        );
        if powered {
            assert_eq!(
                pose::total_scale(&layers[1].previous_pose[body]),
                pose::total_scale(&dying.rest[body])
            );
        }
    }
}
