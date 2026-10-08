use super::super::evaluation::MolangValue;
use super::*;

/// Easing curves retain their small endpoint residuals; poses agree within 0.05 model units/degrees.
fn close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 0.05,
        "{actual} differs from {expected}"
    );
}

/// Checks an authored translation without requiring exact floating-point endpoint cancellation.
fn close_position(actual: [f32; 3], expected: [f32; 3]) {
    for (actual, expected) in actual.into_iter().zip(expected) {
        close(actual, expected);
    }
}

/// A finite kinetic component lets phase endpoints be checked independently of animation ticks.
fn timing() -> protocol::KineticWeaponTiming {
    protocol::KineticWeaponTiming {
        delay_ticks: 12,
        dismount_ticks: 50,
        knockback_ticks: 135,
        damage_ticks: 225,
    }
}

#[test]
fn spear_jab_raises_thrusts_and_recovers_over_the_item_swing() {
    let seconds = 0.95;
    let mut pose = SpearPose::default();
    pose.sample_attack(0.05 / seconds, seconds);
    close_position(pose.fp_attack.position, [4.0, -3.0, 26.0]);
    close(pose.fp_attack.attachable_rotation_z, 40.0);
    close(pose.tp_attack_arm_x, 90.0);
    pose.sample_attack(0.2 / seconds, seconds);
    close_position(pose.fp_attack.position, [0.0, -3.0, 0.0]);
    close(pose.tp_attack_arm_x, -30.0);
    close(pose.tp_attack_item_x, 60.0);
    close(pose.tp_attack_attachable_z, -6.0);
    pose.sample_attack(1.0, seconds);
    close_position(pose.fp_attack.position, [0.0; 3]);
    close(pose.fp_attack.rotation_z, 0.0);
    close(pose.tp_attack_arm_x, 0.0);
    close(pose.tp_attack_attachable_z, 0.0);
}

#[test]
fn spear_charge_uses_component_tick_endpoints_and_returns_to_rest() {
    let mut pose = SpearPose::default();
    let timing = timing();
    pose.sample_use(timing.delay_ticks as f32, timing);
    close_position(pose.fp_use.position, [0.0; 3]);
    close(pose.fp_use.rotation_y, 20.0);
    close(pose.fp_use.rotation_z, -60.0);
    close(pose.fp_use.attachable_rotation_z, -50.0);
    close(pose.tp_use_arm[0], -30.0);
    close(pose.tp_use_attachable_z, 90.0);
    pose.sample_use((timing.delay_ticks + timing.damage_ticks) as f32, timing);
    close_position(pose.fp_use.position, [0.0; 3]);
    close(pose.fp_use.rotation_y, 0.0);
    close(pose.fp_use.rotation_z, 0.0);
    close(pose.fp_use.attachable_rotation_z, 0.0);
    close_position(pose.tp_use_arm, [0.0; 3]);
}

#[test]
fn spear_slots_sample_the_use_fraction_and_clear_when_unequipped() {
    let slots = Slots::new(|name| {
        VARIABLES
            .iter()
            .position(|(candidate, _)| *candidate == name)
    });
    let mut variables = MolangVariables::slots(VARIABLES.len());
    let actor = super::super::tests::actor_with_metadata(HashMap::new());
    let context = ActorTickContext {
        main_hand_is_spear: true,
        main_hand_kinetic: Some(timing()),
        main_hand_swing_seconds: Some(0.95),
        frame_alpha: 0.5,
        ..Default::default()
    };
    let input = ActorTickInput {
        item_use_ticks: 1,
        ..Default::default()
    };
    slots.apply(&mut variables, &actor, &context, &input, 0.0);
    let read = |variables: &MolangVariables, name: &str| {
        variables
            .number_at(
                VARIABLES
                    .iter()
                    .position(|(candidate, _)| *candidate == name)
                    .unwrap(),
            )
            .unwrap()
    };
    assert_eq!(read(&variables, "variable.melee_spear_equipped"), 1.0);
    let halfway = read(&variables, "variable.fp_melee_spear_use_item_position_x");
    assert!(
        halfway > 0.0 && halfway < 0.5,
        "the first use frame is a partial tick, not a whole raise"
    );
    slots.apply(
        &mut variables,
        &actor,
        &ActorTickContext::default(),
        &input,
        0.0,
    );
    assert!(
        VARIABLES
            .iter()
            .enumerate()
            .all(|(slot, (name, _))| variables.number_at(slot)
                == Some(if *name == "variable.tp_melee_spear_base_arm_rotation_x" {
                    -30.0
                } else {
                    0.0
                }))
    );
}

#[test]
fn native_spear_tag_query_uses_the_selected_item_fact() {
    let actor = super::super::tests::actor_with_metadata(HashMap::new());
    let input = ActorTickInput::default();
    let context = ActorTickContext {
        main_hand_is_spear: true,
        ..Default::default()
    };
    let query = |context: &ActorTickContext, slot: &str, tag: &str| {
        query::query(
            &query::QueryInputs {
                actor: &actor,
                input: &input,
                context,
                anim_tick: 0,
                anim_time: None,
                life_tick: 0,
                finished: (false, false),
                bones: &[],
                bone_names: &[],
            },
            "query.equipped_item_any_tag",
            &[
                MolangValue::String(slot.into()),
                MolangValue::String(tag.into()),
            ],
        )
        .number()
    };
    assert_eq!(
        query(&context, "slot.weapon.mainhand", "minecraft:is_spear"),
        1.0
    );
    assert_eq!(
        query(
            &ActorTickContext::default(),
            "slot.weapon.mainhand",
            "minecraft:is_spear"
        ),
        0.0
    );
    assert_eq!(
        query(&context, "slot.weapon.offhand", "minecraft:is_spear"),
        0.0
    );
}

#[test]
fn authored_player_spear_inputs_keep_their_own_pre_animation() {
    let mut compiled = super::super::attachable::tests::compiled_fixture();
    compiled.sources[1].path = "entity/player.json".into();
    compiled.symbols[4].kind = assets::EntityAssetKind::Entity;
    compiled.symbols[4].identifier = "minecraft:player".into();
    compiled.symbols.rotate_right(1);
    compiled.rig_bindings[0].entity_symbol = 0;
    compiled.rig_bindings[0].render_controller = 3;
    compiled.animation_clips[0].symbol = 2;
    compiled.molang_symbols[7].identifier = "variable.melee_spear_equipped".into();
    let assets = RuntimeEntityAssets::from_compiled(compiled).unwrap();
    assert!(
        !needs_completion(&assets, 0),
        "a supplied spear pre-animation must win"
    );
}

#[test]
fn installed_player_spear_thrust_moves_its_arm_and_samples_frames_without_advancing() {
    let path = std::env::var_os("CINNABAR_ENTITY_ASSETS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../.local/assets/compiled/vanilla-v1.mcbeent")
        });
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!(
                "skipping spear player fixture: {} is absent",
                path.display()
            );
            return;
        }
        Err(error) => panic!("read {}: {error}", path.display()),
    };
    let assets = Arc::new(RuntimeEntityAssets::decode(&bytes).unwrap());
    assert!(
        assets
            .symbols()
            .iter()
            .any(|symbol| symbol.kind == assets::EntityAssetKind::Attachable
                && symbol.identifier.as_ref() == "minecraft:iron_spear"),
        "the installed spear model must have an attachable"
    );
    let mut actor = super::super::tests::actor_with_metadata(HashMap::new());
    actor.kind = ActorKind::Player {
        uuid: [1; 16],
        username: "Player".into(),
    };
    let mut store = ActorAnimationStore::with_assets(Arc::clone(&assets));
    store.insert(1, 0, &actor);
    let actors = HashMap::from([(1, actor)]);
    let context = ActorTickContext {
        is_local: true,
        is_local_first_person: true,
        main_hand: Some("minecraft:iron_spear".into()),
        main_hand_is_spear: true,
        main_hand_kinetic: Some(timing()),
        main_hand_swing_seconds: Some(0.95),
        main_hand_max_use_ticks: 1000,
        bedrock_swing_ticks: (0.95 / ACTOR_TICK_DURATION.as_secs_f32()).round() as i32,
        ..Default::default()
    };
    store.advance_tick(&actors, None, Some(1), true, true, |_| context.clone());
    let idle = store.get(1).unwrap().current.to_vec();
    store.sync_local_swing(
        1,
        LocalSwingProgress {
            bedrock: [0.0, 0.2],
            java: [0.0, 0.2],
            frame_alpha: Some(0.5),
        },
    );
    store.refresh_local_view(&actors, 1, |_| context.clone());
    let posed = store.get(1).unwrap();
    assert_ne!(
        posed.current, idle,
        "the spear attack controller must consume a nonzero pose"
    );
    let lifetime = store.runtime_to_lifetime[&1];
    let state = &store.rigs[&lifetime];
    assert!(state.samples_swing_poses && state.complete_spear_variables);
    let slot = store
        .layout
        .named_slot(&assets, "variable.fp_melee_spear_attack_item_position_z")
        .unwrap();
    assert!(state.variables.number_at(slot).unwrap().abs() > 1.0);
    let completed = (store.completed_tick, state.current.clone());
    let input = state.history.back().unwrap();
    let owner = posed.animation_variables;
    let mut sampled = store.layout.fresh(1);
    owner.copy_to(&assets, &store.layout, &mut sampled);
    owner.sample_spear_to(
        &store.layout.engine.spear,
        &mut sampled,
        &actors[&1],
        &ActorTickInput {
            attack_time: 0.05 / 0.95,
            ..*input
        },
        0.5,
    );
    assert_eq!(
        sampled.number_at(slot),
        Some(26.0),
        "the held model observes the owner's frame pose"
    );
    assert_eq!(
        (store.completed_tick, store.rigs[&lifetime].current.clone()),
        completed
    );
}
