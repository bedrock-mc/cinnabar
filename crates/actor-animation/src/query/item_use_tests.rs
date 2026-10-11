use super::*;

/// Evaluates item timing with an original player fixture and explicit use state.
fn remaining(elapsed: u32, duration: u32, arguments: &[MolangValue]) -> f32 {
    remaining_with_off_hand(elapsed, duration, 0, arguments)
}

/// Evaluates slot normalization against one shared player use counter.
fn remaining_with_off_hand(
    elapsed: u32,
    duration: u32,
    off_hand_duration: u32,
    arguments: &[MolangValue],
) -> f32 {
    let mut actor = super::super::tests::actor_with_metadata(HashMap::new());
    actor.kind = protocol::ActorKind::Player {
        uuid: [0; 16],
        username: "BugTest".into(),
    };
    if elapsed > 0 {
        actor
            .metadata
            .insert(0, ActorMetadataValue::Flags(1 << FLAG_USING_ITEM));
    }
    let input = ActorTickInput {
        item_use_ticks: elapsed.saturating_add(1),
        ..Default::default()
    };
    let context = ActorTickContext {
        main_hand: Some("minecraft:crossbow".into()),
        main_hand_max_use_ticks: duration,
        local_item_use_ticks: Some(elapsed),
        off_hand_max_use_ticks: off_hand_duration,
        ..Default::default()
    };
    query(
        &QueryInputs {
            actor: &actor,
            input: &input,
            context: &context,
            anim_tick: 0,
            anim_time: None,
            life_tick: 0,
            finished: (false, false),
            state_time: 0.0,
            swell_amount: None,
            bones: &[],
            bone_names: &[],
        },
        Q::ItemRemainingUseDuration,
        arguments,
    )
    .number()
}

#[test]
fn off_hand_normalization_uses_the_player_counter_and_clamps_the_selected_range() {
    let arguments = [
        MolangValue::String("off_hand".into()),
        MolangValue::Number(2.0),
    ];
    assert_eq!(remaining_with_off_hand(5, 25, 10, &arguments), 2.0);
    assert_eq!(remaining_with_off_hand(5, 25, 40, &arguments), 1.0);
}

#[test]
fn crossbow_remaining_duration_honors_normalization_and_slot_arguments() {
    let slot = MolangValue::String("main_hand".into());
    let range = MolangValue::Number(1.0);
    assert!((remaining(5, 25, &[slot.clone(), range.clone()]) - 0.8).abs() < 1e-6);
    assert!((remaining(5, 25, &[range]) - 0.8).abs() < 1e-6);
    assert!((remaining(5, 25, &[]) - 1.0).abs() < 1e-6);
    assert_eq!(remaining(5, 25, &[slot]), 400.0);
    assert!((remaining(5, 25, &[MolangValue::Number(2.0)]) - 1.6).abs() < 1e-6);
    assert_eq!(
        remaining(5, 25, &[MolangValue::String("invalid".into())]),
        0.0
    );
    assert_eq!(
        remaining(5, 25, &[MolangValue::String("off_hand".into())]),
        0.0
    );
}

#[test]
fn interrupted_and_completed_crossbow_use_has_no_remaining_duration() {
    assert_eq!(remaining(0, 25, &[]), 0.0);
    assert_eq!(remaining(25, 25, &[MolangValue::Number(1.0)]), 0.0);
    assert_eq!(remaining(30, 25, &[MolangValue::Number(1.0)]), 0.0);
    assert_eq!(remaining(5, 0, &[MolangValue::Number(1.0)]), 0.0);
}

/// Loads the installed authored player rig without redistributing its carrier.
fn installed_assets() -> Option<Arc<RuntimeEntityAssets>> {
    let configured = std::env::var_os("CINNABAR_ENTITY_ASSETS");
    let required = configured.is_some();
    let path = configured.map(std::path::PathBuf::from).unwrap_or_else(|| {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(assets::carriers::COMPILED_DIR)
            .join(assets::carriers::ENTITY.output)
    });
    match std::fs::read(&path) {
        Ok(bytes) => Some(Arc::new(RuntimeEntityAssets::decode(&bytes).unwrap())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && !required => {
            eprintln!(
                "skipping crossbow parent pose: missing fixture {}",
                path.display()
            );
            None
        }
        Err(error) => panic!("read {}: {error}", path.display()),
    }
}

/// Samples the real player parent with a caller-owned use clock and fixed actor age.
fn parent_pose(
    assets: Arc<RuntimeEntityAssets>,
    elapsed: u32,
    duration: u32,
    charged: bool,
) -> BoneTransform {
    let mut actor = super::super::tests::actor_with_metadata(HashMap::new());
    actor.kind = protocol::ActorKind::Player {
        uuid: [0; 16],
        username: "BugTest".into(),
    };
    if elapsed > 0 {
        actor
            .metadata
            .insert(0, ActorMetadataValue::Flags(1 << FLAG_USING_ITEM));
    }
    let mut store = ActorAnimationStore::with_assets(assets);
    store.insert(1, 0, &actor);
    let actors = HashMap::from([(1, actor)]);
    for _ in 0..4 {
        store.advance_tick(&actors, None, Some(1), true, true, |_| ActorTickContext {
            is_local: true,
            is_local_first_person: true,
            main_hand: Some("minecraft:crossbow".into()),
            main_hand_max_use_ticks: duration,
            local_item_use_ticks: Some(elapsed),
            hand_charged: charged,
            ..Default::default()
        });
    }
    let rig = store.get(1).unwrap();
    let parent = rig
        .bone_names
        .iter()
        .position(|name| name.as_ref() == "rightitem")
        .unwrap();
    rig.current[parent]
}

#[test]
fn installed_crossbow_parent_moves_while_charging_and_normalizes_charge_rates() {
    let Some(assets) = installed_assets() else {
        return;
    };
    let idle = parent_pose(Arc::clone(&assets), 0, 24, false);
    let charging = parent_pose(Arc::clone(&assets), 12, 24, false);
    assert_ne!(
        charging, idle,
        "charging must move the authored player parent, not only its item texture"
    );
    assert_eq!(
        charging,
        parent_pose(Arc::clone(&assets), 4, 8, false),
        "equal charge fractions use the same parent pose at different charge rates"
    );
    assert_eq!(
        idle,
        parent_pose(assets, 0, 24, true),
        "a loaded idle crossbow must leave the pulling pose"
    );
}
