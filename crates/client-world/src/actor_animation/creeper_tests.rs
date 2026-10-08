use super::*;

#[test]
fn installed_creeper_samples_pack_swelling_and_flash_between_ticks() {
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
                value: ActorMetadataValue::Flags(1 << 10),
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
}
