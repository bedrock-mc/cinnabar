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
    let mut store = crate::actor_store::ActorStore::new_with_entity_assets(1, 0, assets);
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
    let tick = store.actor_rig(1).unwrap().completed_tick;
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
    store.advance_interpolation_ticks(40);
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
}
