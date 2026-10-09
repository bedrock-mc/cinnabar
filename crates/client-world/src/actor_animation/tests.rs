use super::{evaluation::MolangValue, pose::LocalDelta, query::QueryInputs, *};
use crate::ActorPose;

#[test]
fn item_animation_interpolates_observations_before_rendering_and_wraps_the_swing() {
    let previous = ItemAnimationState {
        attack_time: 5.0 / 6.0,
        arm_height: 0.2,
    };
    let current = ItemAnimationState {
        attack_time: 0.0,
        arm_height: 0.8,
    };
    let halfway = previous.interpolate(current, 0.5);
    assert!((halfway.attack_time - 11.0 / 12.0).abs() < 1e-6);
    assert!((halfway.arm_height - 0.5).abs() < 1e-6);
    assert_eq!(previous.interpolate(current, 0.0), previous);
}

#[test]
fn static_generation_exhaustion_fails_closed_without_consuming_animated_generation() {
    let mut store = ActorAnimationStore::diagnostic();
    let animated = store.next_reset_generation;
    store.next_rest_reset_generation = u64::MAX - 1;
    assert_eq!(store.take_rest_generation(), Some(u64::MAX - 1));
    assert_eq!(store.take_rest_generation(), None);
    assert_eq!(store.take_rest_generation(), None);
    assert_eq!(store.next_reset_generation, animated);
    store.clear();
    assert_eq!(store.take_rest_generation(), None);
}

pub(super) fn actor_with_metadata(metadata: HashMap<u32, ActorMetadataValue>) -> ActorSnapshot {
    let pose = ActorPose {
        position: [0.0; 3],
        pitch: 0.0,
        yaw: 0.0,
        head_yaw: 0.0,
    };
    ActorSnapshot {
        unique_id: -1,
        runtime_id: 1,
        spawn_revision: 1,
        movement_revision: 0,
        kind: ActorKind::Entity {
            identifier: "minecraft:test".into(),
        },
        position: [0.0; 3],
        velocity: [0.0; 3],
        pitch: 0.0,
        yaw: 0.0,
        head_yaw: 0.0,
        previous_pose: pose,
        received_pose: pose,
        interpolation_ticks_remaining: 0,
        body_yaw: 0.0,
        on_ground: Some(false),
        teleported: false,
        player_mode: None,
        player_game_mode: None,
        source_tick: None,
        metadata,
        attributes: HashMap::new(),
        int_properties: HashMap::new(),
        float_properties: HashMap::new(),
        status: Default::default(),
        dragon_animation: None,
    }
}

fn read(actor: &ActorSnapshot, input: &ActorTickInput, life_tick: u64, name: &str) -> f32 {
    read_with(
        actor,
        input,
        &ActorTickContext::default(),
        life_tick,
        name,
        &[],
    )
    .number()
}

fn read_with(
    actor: &ActorSnapshot,
    input: &ActorTickInput,
    context: &ActorTickContext,
    life_tick: u64,
    name: &str,
    arguments: &[MolangValue],
) -> MolangValue {
    let inputs = QueryInputs {
        actor,
        input,
        context,
        anim_tick: 0,
        anim_time: None,
        swell_amount: None,
        life_tick,
        finished: (false, false),
        bones: &[],
        bone_names: &[],
    };
    query::query(&inputs, name, arguments)
}

#[test]
fn child_before_parent_composes_without_reindexing_channels() {
    let bones = [
        RuntimeBone {
            parent: Some(1),
            pivot: [0.0, 2.0, 0.0],
            rotation: [0.0; 3],
            ..RuntimeBone::default()
        },
        RuntimeBone {
            parent: None,
            pivot: [1.0, 0.0, 0.0],
            rotation: [0.0; 3],
            ..RuntimeBone::default()
        },
    ];
    let pose = compose_pose(&bones, &[]).unwrap();
    assert_eq!(pose[0].translation_scale[0..3], [0.0, 2.0, 0.0]);
    assert_eq!(pose[1].translation_scale[0..3], [1.0, 0.0, 0.0]);
}

#[test]
fn rotated_parent_uses_child_model_space_pivot_delta() {
    let bones = [
        RuntimeBone {
            parent: None,
            pivot: [1.0, 0.0, 0.0],
            rotation: [0.0, 0.0, 90.0],
            ..RuntimeBone::default()
        },
        RuntimeBone {
            parent: Some(0),
            pivot: [3.0, 0.0, 0.0],
            rotation: [0.0; 3],
            ..RuntimeBone::default()
        },
    ];
    let pose = compose_pose(&bones, &[]).unwrap();
    assert!((pose[1].translation_scale[0] - 1.0).abs() < 1.0e-5);
    assert!((pose[1].translation_scale[1] - 2.0).abs() < 1.0e-5);
}

#[test]
fn entity_relative_rotation_keeps_the_parented_pivot_and_resets_the_basis() {
    let bones = [
        RuntimeBone {
            pivot: [1.0, 2.0, 0.0],
            rotation: [28.0, 40.0, 15.0],
            ..RuntimeBone::default()
        },
        RuntimeBone {
            parent: Some(0),
            pivot: [1.0, 4.0, 0.0],
            rotation: [-20.0, 30.0, -10.0],
            ..RuntimeBone::default()
        },
        RuntimeBone {
            parent: Some(1),
            pivot: [1.0, 5.0, 0.0],
            ..RuntimeBone::default()
        },
    ];
    let mut local = [
        LocalDelta {
            scale: [2.0; 3],
            ..LocalDelta::default()
        },
        LocalDelta {
            translation: [0.0, -1.0, 0.0],
            scale: [3.0; 3],
            ..LocalDelta::default()
        },
        LocalDelta::default(),
    ];
    let inherited = compose_pose(&bones, &local).unwrap();
    local[1].rotation_relative_to_entity = true;
    let relative = compose_pose(&bones, &local).unwrap();
    assert_eq!(
        relative[1].translation_scale[..3],
        inherited[1].translation_scale[..3]
    );
    assert_eq!(relative[1].translation_scale[3], 3.0);
    assert_eq!(relative[2].translation_scale[3], 3.0);
    let unparented = RuntimeBone {
        parent: None,
        ..bones[1]
    };
    let own = compose_pose(&[unparented], &[local[1]]).unwrap();
    assert_eq!(relative[1].rotation, own[0].rotation);
    assert_eq!(relative[2].rotation, relative[1].rotation);
    assert_ne!(relative[1].rotation, inherited[1].rotation);
}

#[test]
fn nonuniform_scale_is_carried_per_axis_and_inherited_by_children() {
    let bones = [
        RuntimeBone {
            parent: None,
            pivot: [0.0; 3],
            rotation: [0.0; 3],
            ..RuntimeBone::default()
        },
        RuntimeBone {
            parent: Some(0),
            pivot: [2.0, 4.0, 0.0],
            rotation: [0.0; 3],
            ..RuntimeBone::default()
        },
    ];
    let local = [
        LocalDelta {
            scale: [1.0, 0.5, 1.0],
            ..LocalDelta::default()
        },
        LocalDelta {
            scale: [2.0; 3],
            ..LocalDelta::default()
        },
    ];
    let pose = compose_pose(&bones, &local).unwrap();
    assert_eq!(pose[0].axis_scale, [1.0, 0.5, 1.0]);
    assert_eq!(pose[0].translation_scale[3], 1.0);
    assert_eq!(pose[1].axis_scale, [2.0, 1.0, 2.0]);
    assert_eq!(
        pose[1].translation_scale[0..2],
        [2.0, 2.0],
        "child offset squashed in Y"
    );
    let uniform = compose_pose(&bones, &[LocalDelta::default(), local[1]]).unwrap();
    assert_eq!(
        (uniform[1].translation_scale[3], uniform[1].axis_scale),
        (2.0, [1.0; 3])
    );
}

#[test]
fn sleeping_player_metadata_does_not_spoof_sneaking() {
    let actor = actor_with_metadata(HashMap::from([(26, ActorMetadataValue::Byte(2))]));
    let input = ActorTickInput::default();
    assert_eq!(read(&actor, &input, 0, "query.is_sleeping"), 1.0);
    assert_eq!(read(&actor, &input, 0, "query.is_sneaking"), 0.0);
}

#[test]
fn riding_flag_is_not_read_as_sleeping() {
    let actor = actor_with_metadata(HashMap::from([(0, ActorMetadataValue::Flags(1 << 2))]));
    assert_eq!(
        read(&actor, &ActorTickInput::default(), 0, "query.is_sleeping"),
        0.0
    );
    let sleeping = actor_with_metadata(HashMap::from([(
        92,
        ActorMetadataValue::FlagsExtended(1 << (76 - 64)),
    )]));
    assert_eq!(
        read(
            &sleeping,
            &ActorTickInput::default(),
            0,
            "query.is_sleeping"
        ),
        1.0
    );
}

#[test]
fn animation_reset_clock_is_distinct_from_actor_lifetime() {
    let actor = actor_with_metadata(HashMap::new());
    let input = ActorTickInput::default();
    assert_eq!(read(&actor, &input, 7, "query.anim_time"), 0.0);
    assert!((read(&actor, &input, 7, "query.life_time") - 0.35).abs() < 1.0e-6);
}

#[test]
fn riding_query_reads_only_the_authoritative_tick_input() {
    let actor = actor_with_metadata(HashMap::new());
    let mut input = ActorTickInput {
        is_riding: true,
        ..ActorTickInput::default()
    };
    assert_eq!(read(&actor, &input, 0, "query.is_riding"), 1.0);
    input.is_riding = false;
    assert_eq!(read(&actor, &input, 0, "query.is_riding"), 0.0);
}

#[test]
fn head_target_yaw_is_relative_to_the_body_and_wrapped() {
    let actor = actor_with_metadata(HashMap::new());
    let input = ActorTickInput {
        body_yaw: 170.0,
        head_yaw: -170.0,
        pitch: 30.0,
        ..ActorTickInput::default()
    };
    assert!((read(&actor, &input, 0, "query.target_y_rotation") - 20.0).abs() < 1.0e-4);
    assert_eq!(read(&actor, &input, 0, "query.target_x_rotation"), 30.0);
}

#[test]
fn arrow_target_rotation_is_absolute_and_does_not_follow_a_mob_body() {
    let mut arrow = actor_with_metadata(HashMap::new());
    arrow.kind = ActorKind::Entity {
        identifier: "minecraft:arrow".into(),
    };
    let input = ActorTickInput {
        yaw: 150.0,
        head_yaw: -20.0,
        body_yaw: 70.0,
        pitch: -35.0,
        ..ActorTickInput::default()
    };
    assert_eq!(read(&arrow, &input, 0, "query.target_y_rotation"), 150.0);
    assert_eq!(read(&arrow, &input, 0, "query.target_x_rotation"), -35.0);
}

#[test]
fn shake_time_query_reads_the_signed_retained_native_countdown() {
    let mut arrow = actor_with_metadata(HashMap::new());
    arrow.kind = ActorKind::Entity {
        identifier: "minecraft:arrow".into(),
    };
    for ticks in [i32::MIN, -1, 0, 1, 12, i32::MAX] {
        arrow.status.shake_time = ticks;
        assert_eq!(
            read(&arrow, &ActorTickInput::default(), 0, "query.shake_time"),
            ticks as f32
        );
    }
}

#[test]
fn operation_work_and_transition_budgets_are_aggregate() {
    let mut world_left = 1;
    let mut budget = EvalBudget {
        actor_left: 2,
        world_left: &mut world_left,
        work_left: 1,
        transitions_left: MAX_CONTROLLER_TRANSITIONS_PER_TICK,
        used: 0,
        stack: Vec::new(),
        static_draw: None,
    };
    assert_eq!(budget.charge(), Ok(()));
    assert_eq!(budget.charge(), Err(EvalError::WorldBudget));
    assert_eq!(budget.charge_work(), Ok(()));
    assert_eq!(budget.charge_work(), Err(EvalError::ActorBudget));
    for _ in 0..MAX_CONTROLLER_TRANSITIONS_PER_TICK {
        assert!(budget.take_transition());
    }
    assert!(!budget.take_transition());
}

#[test]
fn item_and_vehicle_queries_read_equipment_and_links_with_vanilla_argument_forms() {
    let actor = actor_with_metadata(HashMap::new());
    let input = ActorTickInput::default();
    let context = ActorTickContext {
        main_hand: Some("minecraft:crossbow".into()),
        off_hand: Some("minecraft:shield".into()),
        ridden: Some("minecraft:boat".into()),
        ..ActorTickContext::default()
    };
    let text = |value: &str| MolangValue::String(value.into());
    let read = |name: &str, arguments: &[MolangValue]| {
        read_with(&actor, &input, &context, 0, name, arguments)
    };
    assert_eq!(read("query.get_equipped_item_name", &[]), text("crossbow"));
    assert_eq!(
        read("query.get_equipped_item_name", &[text("off_hand")]),
        text("shield")
    );
    assert_eq!(
        read("query.get_equipped_item_name", &[MolangValue::Number(1.0)]),
        text("shield")
    );
    let empty = ActorTickContext::default();
    assert_eq!(
        read_with(
            &actor,
            &input,
            &empty,
            0,
            "query.get_equipped_item_name",
            &[]
        ),
        text("")
    );
    let any = |arguments: &[MolangValue]| read("query.is_item_name_any", arguments).number();
    assert_eq!(
        any(&[text("slot.weapon.mainhand"), text("minecraft:crossbow")]),
        1.0
    );
    assert_eq!(
        any(&[
            text("slot.weapon.offhand"),
            MolangValue::Number(0.0),
            text("minecraft:bow"),
            text("minecraft:shield"),
        ]),
        1.0
    );
    assert_eq!(any(&[text("slot.weapon.mainhand"), text("crossbow")]), 0.0);
    let riding = |name: &str| {
        read(
            "query.is_riding_any_entity_of_type",
            &[text("minecraft:minecart"), text(name)],
        )
        .number()
    };
    assert_eq!(riding("minecraft:boat"), 1.0);
    assert_eq!(riding("minecraft:strider"), 0.0);
}

#[test]
fn bare_head_rotation_queries_read_zero_and_limited_forms_clamp() {
    let actor = actor_with_metadata(HashMap::new());
    let input = ActorTickInput {
        head_yaw: 60.0,
        pitch: 12.0,
        ..ActorTickInput::default()
    };
    let context = ActorTickContext::default();
    let read = |name: &str, arguments: &[MolangValue]| {
        read_with(&actor, &input, &context, 0, name, arguments).number()
    };
    assert_eq!(read("query.head_y_rotation", &[]), 0.0);
    assert_eq!(
        read("query.head_y_rotation", &[MolangValue::Number(30.0)]),
        30.0
    );
    assert_eq!(
        read("query.head_x_rotation", &[MolangValue::Number(0.0)]),
        12.0
    );
    assert_eq!(read("query.target_y_rotation", &[]), 60.0);
}

#[test]
fn is_first_person_variable_tracks_the_local_camera_context() {
    let engine = EngineSlots {
        is_first_person: Some(0),
        ..EngineSlots::default()
    };
    let actor = actor_with_metadata(HashMap::new());
    let input = ActorTickInput::default();
    let motion = MotionState::default();
    let mut variables = MolangVariables::slots(1);
    let first_person = ActorTickContext {
        is_local_first_person: true,
        ..ActorTickContext::default()
    };
    tick::apply_engine_variables(
        &engine,
        &mut variables,
        &actor,
        &first_person,
        &input,
        &motion,
    );
    assert_eq!(variables.number_at(0), Some(1.0));
    let third_person = ActorTickContext::default();
    tick::apply_engine_variables(
        &engine,
        &mut variables,
        &actor,
        &third_person,
        &input,
        &motion,
    );
    assert_eq!(variables.number_at(0), Some(0.0));
}

#[test]
fn first_person_driver_variables_track_pitch_and_enable_view_bob() {
    let engine = EngineSlots {
        player_x_rotation: Some(0),
        bob_animation: Some(1),
        ..EngineSlots::default()
    };
    let actor = actor_with_metadata(HashMap::new());
    let input = ActorTickInput {
        pitch: 30.0,
        ..ActorTickInput::default()
    };
    let motion = MotionState::default();
    let mut variables = MolangVariables::slots(2);
    tick::apply_engine_variables(
        &engine,
        &mut variables,
        &actor,
        &ActorTickContext::default(),
        &input,
        &motion,
    );
    assert_eq!(variables.number_at(0), Some(30.0));
    assert_eq!(variables.number_at(1), Some(1.0));
    let context = ActorTickContext {
        is_in_ui: true,
        ..Default::default()
    };
    tick::apply_engine_variables(&engine, &mut variables, &actor, &context, &input, &motion);
    assert_eq!(variables.number_at(0), Some(0.0));
    assert_eq!(
        read_with(&actor, &input, &context, 0, "query.is_in_ui", &[]).number(),
        1.0
    );
    assert_eq!(
        read_with(
            &actor,
            &input,
            &ActorTickContext::default(),
            0,
            "query.is_in_ui",
            &[]
        )
        .number(),
        0.0
    );
}

#[test]
fn use_item_and_headgear_queries_read_their_neutral_idle_values() {
    let actor = actor_with_metadata(HashMap::new());
    let input = ActorTickInput::default();
    for name in [
        "query.main_hand_item_use_duration",
        "query.main_hand_item_max_duration",
        "query.item_remaining_use_duration",
        "query.has_head_gear",
        "query.is_spectator",
    ] {
        assert_eq!(read(&actor, &input, 0, name), 0.0, "{name}");
    }
}

#[test]
fn loop_counts_run_their_ceiling_up_to_the_bound_and_skip_when_not_positive() {
    use evaluation::loop_iterations;
    assert_eq!(loop_iterations(2.5), Some(3));
    assert_eq!(
        loop_iterations(3000.0),
        Some(assets::MAX_MOLANG_LOOP_ITERATIONS)
    );
    assert_eq!(loop_iterations(0.0), None);
    assert_eq!(loop_iterations(-1.0), None);
    assert_eq!(loop_iterations(f32::NAN), None);
}

// Camera-facing holograms initialise from these; without them the whole script was dropped.
#[test]
fn camera_relative_queries_aim_at_the_fed_camera_position() {
    let input = ActorTickInput {
        position: [0.0, 64.0, 0.0],
        ..ActorTickInput::default()
    };
    let context = ActorTickContext {
        camera_position: [3.0, 68.0, 0.0],
        ..ActorTickContext::default()
    };
    let actor = actor_with_metadata(HashMap::new());
    let number = |name: &str, arguments: &[MolangValue]| {
        read_with(&actor, &input, &context, 0, name, arguments).number()
    };
    assert!((number("query.distance_from_camera", &[]) - 5.0).abs() < 1e-5);
    let pitch = number("query.rotation_to_camera", &[MolangValue::Number(0.0)]);
    assert!(
        (pitch + 4.0_f32.atan2(3.0).to_degrees()).abs() < 1e-3,
        "{pitch}"
    );
    let yaw = number("query.rotation_to_camera", &[MolangValue::Number(1.0)]);
    assert!((yaw + 90.0).abs() < 1e-3, "{yaw}");
}

#[test]
fn camera_rotation_reads_the_fed_view_and_xp_orb_frames_follow_value() {
    let input = ActorTickInput::default();
    let context = ActorTickContext {
        camera_rotation: [12.0, -34.0],
        ..ActorTickContext::default()
    };
    let mut orb = actor_with_metadata(HashMap::from([(15, ActorMetadataValue::Int(20))]));
    let number = |actor: &ActorSnapshot, name: &str, arguments: &[MolangValue]| {
        read_with(actor, &input, &context, 0, name, arguments)
    };
    assert_eq!(
        number(&orb, "query.camera_rotation", &[MolangValue::Number(0.0)]),
        MolangValue::Number(12.0)
    );
    assert_eq!(
        number(&orb, "query.camera_rotation", &[MolangValue::Number(1.0)]),
        MolangValue::Number(-34.0)
    );
    // Only orbs pick a frame by value.
    assert_eq!(
        number(&orb, "query.texture_frame_index", &[]),
        MolangValue::Number(0.0)
    );
    orb.kind = ActorKind::Entity {
        identifier: "minecraft:xp_orb".into(),
    };
    assert_eq!(
        number(&orb, "query.texture_frame_index", &[]),
        MolangValue::Number(3.0)
    );
    orb.metadata.insert(15, ActorMetadataValue::Int(5_000));
    assert_eq!(
        number(&orb, "query.texture_frame_index", &[]),
        MolangValue::Number(10.0)
    );
}

#[test]
fn xp_orb_unknown_value_variant_keeps_the_lowest_sprite() {
    let input = ActorTickInput::default();
    let mut orb = actor_with_metadata(HashMap::new());
    orb.kind = ActorKind::Entity {
        identifier: "minecraft:xp_orb".into(),
    };
    for value in [
        ActorMetadataValue::Byte(100),
        ActorMetadataValue::Short(5_000),
        ActorMetadataValue::Long(5_000),
        ActorMetadataValue::Float(5_000.0),
    ] {
        orb.metadata.insert(15, value);
        assert_eq!(
            read(&orb, &input, 0, "query.texture_frame_index"),
            0.0,
            "a well-formed unexpected metadata type has no XP sprite value"
        );
    }
    orb.metadata.insert(15, ActorMetadataValue::Int(5_000));
    assert_eq!(read(&orb, &input, 0, "query.texture_frame_index"), 10.0);
}

#[test]
fn hurt_queries_read_the_client_countdown() {
    let input = ActorTickInput::default();
    let context = ActorTickContext::default();
    let mut actor = actor_with_metadata(HashMap::new());
    actor.status.hurt_time = 7;
    actor.status.hurt_direction = Some(3.0);
    let read = |name: &str| read_with(&actor, &input, &context, 0, name, &[]);
    assert_eq!(read("query.hurt_time"), MolangValue::Number(7.0));
    assert_eq!(read("query.hurt_direction"), MolangValue::Number(3.0));
}

#[test]
fn state_queries_read_target_swell_shield_and_death() {
    let mut actor = actor_with_metadata(HashMap::from([
        (6, ActorMetadataValue::Long(42)),
        (19, ActorMetadataValue::Int(14)),
    ]));
    let input = ActorTickInput::default();
    assert_eq!(read(&actor, &input, 0, "query.has_target"), 1.0);
    assert_eq!(read(&actor, &input, 0, "query.swell_amount"), 0.5);
    actor.metadata.insert(6, ActorMetadataValue::Long(-1));
    assert_eq!(read(&actor, &input, 0, "query.has_target"), 0.0);
    actor.status.death_time = 9;
    assert_eq!(read(&actor, &input, 0, "query.death_ticks"), 9.0);
    assert_eq!(read(&actor, &input, 0, "query.is_shield_powered"), 0.0);
    actor.attributes.insert(
        "minecraft:health".into(),
        protocol::ActorAttribute {
            name: "minecraft:health".into(),
            min: 0.0,
            max: 300.0,
            current: 150.0,
            default: None,
            modifiers: Arc::from([]),
        },
    );
    assert_eq!(read(&actor, &input, 0, "query.is_shield_powered"), 1.0);
}

#[test]
fn creeper_ignition_advances_and_reverses_swelling_without_swell_metadata() {
    let mut store = crate::actor_store::ActorStore::new(1, 0);
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
    let input = ActorTickInput::default();
    store.advance_interpolation_ticks(14);
    let actor = store.get(1).unwrap();
    assert_eq!(read(actor, &input, 0, "query.swell_amount"), 13.0 / 28.0);
    assert_eq!(read(actor, &input, 0, "query.swelling_dir"), 1.0);
    let context = ActorTickContext {
        frame_alpha: 0.5,
        ..Default::default()
    };
    assert_eq!(
        read_with(actor, &input, &context, 0, "query.swell_amount", &[]).number(),
        13.5 / 28.0
    );
    store.apply(
        1,
        2,
        protocol::ActorEvent::Metadata(protocol::ActorMetadataUpdateEvent {
            dimension: 0,
            runtime_id: 1,
            metadata: Arc::from([protocol::ActorMetadata {
                key: 0,
                value: ActorMetadataValue::Flags(0),
            }]),
            properties: Arc::from([]),
            tick: 14,
        }),
    );
    store.advance_interpolation_ticks(3);
    let actor = store.get(1).unwrap();
    assert_eq!(read(actor, &input, 0, "query.swell_amount"), 12.0 / 28.0);
    assert_eq!(read(actor, &input, 0, "query.swelling_dir"), -1.0);
    store.advance_interpolation_ticks(100);
    assert_eq!(
        read(store.get(1).unwrap(), &input, 0, "query.swell_amount"),
        0.0
    );
    store.apply(
        1,
        3,
        protocol::ActorEvent::Metadata(protocol::ActorMetadataUpdateEvent {
            dimension: 0,
            runtime_id: 1,
            metadata: Arc::from([
                protocol::ActorMetadata {
                    key: 0,
                    value: ActorMetadataValue::Flags(1 << 10),
                },
                protocol::ActorMetadata {
                    key: 19,
                    value: ActorMetadataValue::Int(5_000),
                },
                protocol::ActorMetadata {
                    key: 21,
                    value: ActorMetadataValue::Int(-5_000),
                },
            ]),
            properties: Arc::from([]),
            tick: 117,
        }),
    );
    store.advance_interpolation_ticks(100);
    assert_eq!(
        read(store.get(1).unwrap(), &input, 0, "query.swell_amount"),
        30.0 / 28.0
    );
    store.apply(
        1,
        4,
        protocol::ActorEvent::Status(protocol::ActorStatusEvent {
            runtime_id: 1,
            kind: protocol::ActorStatusKind::Death,
            data: 0,
        }),
    );
    store.advance_interpolation_ticks(2);
    assert_eq!(
        read(store.get(1).unwrap(), &input, 0, "query.swell_amount"),
        0.0
    );
}

#[test]
fn item_use_duration_counts_seconds_and_water_follows_swimming_or_aquatic_airborne() {
    let actor = actor_with_metadata(HashMap::new());
    let input = ActorTickInput {
        item_use_ticks: 20,
        ..ActorTickInput::default()
    };
    assert_eq!(
        read(&actor, &input, 0, "query.main_hand_item_use_duration"),
        1.0
    );
    assert_eq!(read(&actor, &input, 0, "query.is_in_water"), 0.0);
    let swimmer = actor_with_metadata(HashMap::from([(0, ActorMetadataValue::Flags(1 << 57))]));
    assert_eq!(read(&swimmer, &input, 0, "query.is_in_water"), 1.0);
    let mut fish = actor_with_metadata(HashMap::new());
    fish.kind = ActorKind::Entity {
        identifier: "minecraft:cod".into(),
    };
    assert_eq!(read(&fish, &input, 0, "query.is_in_water"), 1.0);
    let grounded = ActorTickInput {
        on_ground: true,
        ..input
    };
    assert_eq!(read(&fish, &grounded, 0, "query.is_in_water"), 0.0);
}

#[test]
fn sampled_fluid_overrides_the_heuristic_and_armor_queries_read_worn_stacks() {
    let mut actor = actor_with_metadata(HashMap::new());
    let input = ActorTickInput::default();
    actor.status.fluid = Some((false, true));
    assert_eq!(read(&actor, &input, 0, "query.is_in_water"), 0.0);
    assert_eq!(read(&actor, &input, 0, "query.is_in_lava"), 1.0);
    let mut context = ActorTickContext::default();
    context.armor[4] = Some(WornArmor {
        item: "minecraft:golden_horse_armor".into(),
        dye_rgb: None,
    });
    context.armor[0] = Some(WornArmor {
        item: "minecraft:leather_helmet".into(),
        dye_rgb: Some(0x00FF_0000),
    });
    let number = |name: &str, arguments: &[f32]| {
        let arguments: Vec<_> = arguments
            .iter()
            .map(|value| MolangValue::Number(*value))
            .collect();
        read_with(&actor, &input, &context, 0, name, &arguments).number()
    };
    assert_eq!(number("query.armor_texture_slot", &[4.0]), 3.0);
    assert_eq!(number("query.armor_texture_slot", &[1.0]), 0.0);
    assert_eq!(number("query.armor_color_slot", &[0.0, 0.0]), 1.0);
    assert_eq!(number("query.armor_color_slot", &[0.0, 1.0]), 0.0);
}

#[test]
fn item_duration_queries_report_max_and_remaining_seconds() {
    let actor = actor_with_metadata(HashMap::new());
    let input = ActorTickInput {
        item_use_ticks: 20,
        ..ActorTickInput::default()
    };
    let context = ActorTickContext {
        main_hand_max_use_ticks: 32,
        ..ActorTickContext::default()
    };
    let number = |name: &str| read_with(&actor, &input, &context, 0, name, &[]).number();
    assert!((number("query.main_hand_item_use_duration") - 1.0).abs() < 1e-6);
    assert!((number("query.main_hand_item_max_duration") - 1.6).abs() < 1e-6);
    assert!((number("query.item_remaining_use_duration") - 0.6).abs() < 1e-6);
    let unknown = ActorTickContext::default();
    assert_eq!(
        read_with(
            &actor,
            &input,
            &unknown,
            0,
            "query.main_hand_item_max_duration",
            &[]
        )
        .number(),
        0.0
    );
}

#[test]
fn charged_hand_and_bed_rotation_queries_read_their_feeds() {
    let mut actor = actor_with_metadata(HashMap::new());
    let input = ActorTickInput::default();
    assert_eq!(read(&actor, &input, 0, "query.item_is_charged"), 0.0);
    assert_eq!(read(&actor, &input, 0, "query.sleep_rotation"), 0.0);
    actor.status.sleep_rotation = Some(90.0);
    assert_eq!(read(&actor, &input, 0, "query.sleep_rotation"), 90.0);
    let context = ActorTickContext {
        hand_charged: true,
        ..ActorTickContext::default()
    };
    let charged = read_with(&actor, &input, &context, 0, "query.item_is_charged", &[]);
    assert_eq!(charged.number(), 1.0);
}

#[test]
fn property_query_resolves_names_against_synced_definitions() {
    use crate::actor_store::properties::{PropertyDefinition, PropertyKind};
    let mut actor = actor_with_metadata(HashMap::new());
    actor.int_properties.insert(0, 1);
    actor.int_properties.insert(1, 1);
    actor.float_properties.insert(2, 0.5);
    let context = ActorTickContext {
        properties: Some(Arc::from([
            PropertyDefinition {
                name: "minecraft:angry".into(),
                kind: PropertyKind::Number,
                default: 0.0,
            },
            PropertyDefinition {
                name: "minecraft:variant".into(),
                kind: PropertyKind::Enum(Arc::from([Arc::from("pale"), Arc::from("ashen")])),
                default: 0.0,
            },
            PropertyDefinition {
                name: "minecraft:amount".into(),
                kind: PropertyKind::Number,
                default: 0.0,
            },
        ])),
        ..ActorTickContext::default()
    };
    let input = ActorTickInput::default();
    let read = |name: &str| {
        let argument = [MolangValue::String(name.into())];
        read_with(&actor, &input, &context, 0, "query.property", &argument)
    };
    assert_eq!(read("minecraft:angry"), MolangValue::Number(1.0));
    assert_eq!(
        read("minecraft:variant"),
        MolangValue::String("ashen".into())
    );
    assert_eq!(read("minecraft:amount"), MolangValue::Number(0.5));
    assert_eq!(read("minecraft:missing"), MolangValue::Number(0.0));
}

#[test]
fn has_cape_reads_the_tick_context() {
    let actor = actor_with_metadata(HashMap::new());
    let input = ActorTickInput::default();
    let capeless = read_with(
        &actor,
        &input,
        &ActorTickContext::default(),
        0,
        "query.has_cape",
        &[],
    );
    assert_eq!(capeless.number(), 0.0);
    let context = ActorTickContext {
        has_cape: true,
        ..ActorTickContext::default()
    };
    assert_eq!(
        read_with(&actor, &input, &context, 0, "query.has_cape", &[]).number(),
        1.0
    );
}

#[test]
fn elytra_reads_slot_five_on_the_chest_and_cape_flap_follows_ground_speed() {
    let actor = actor_with_metadata(HashMap::new());
    let mut context = ActorTickContext::default();
    context.armor[1] = Some(WornArmor {
        item: "minecraft:elytra".into(),
        dye_rgb: None,
    });
    let input = ActorTickInput {
        velocity: [0.0, 0.0, 0.1],
        ..ActorTickInput::default()
    };
    let slot = read_with(
        &actor,
        &input,
        &context,
        0,
        "query.armor_texture_slot",
        &[MolangValue::Number(1.0)],
    );
    assert_eq!(slot.number(), 5.0);
    let flap = read_with(&actor, &input, &context, 0, "query.cape_flap_amount", &[]);
    assert!((flap.number() - 0.4).abs() < 1.0e-6);
}

// The first-person item offset reads rest pivots; rig pivots mirror authored X.
#[test]
fn default_bone_pivot_reads_the_authored_rest_pivot() {
    let actor = actor_with_metadata(HashMap::new());
    let input = ActorTickInput::default();
    let context = ActorTickContext::default();
    let bones = [RuntimeBone {
        parent: None,
        pivot: [5.0, 22.0, 1.0],
        rotation: [0.0; 3],
        ..RuntimeBone::default()
    }];
    let names = [Box::<str>::from("rightarm")];
    let inputs = QueryInputs {
        actor: &actor,
        input: &input,
        context: &context,
        anim_tick: 0,
        anim_time: None,
        swell_amount: None,
        life_tick: 0,
        finished: (false, false),
        bones: &bones,
        bone_names: &names,
    };
    let pivot = |name: &str, axis: f32| {
        query::query(
            &inputs,
            "query.get_default_bone_pivot",
            &[MolangValue::String(name.into()), MolangValue::Number(axis)],
        )
        .number()
    };
    assert_eq!(pivot("rightArm", 0.0), -5.0);
    assert_eq!(pivot("rightArm", 1.0), 22.0);
    assert_eq!(pivot("missing", 1.0), 0.0);
}

#[test]
fn arrow_target_yaw_uses_interpolated_absolute_rotation_not_the_latest_packet() {
    let mut actor = actor_with_metadata(HashMap::new());
    actor.kind = ActorKind::Entity {
        identifier: "minecraft:arrow".into(),
    };
    actor.yaw = 135.0;
    let mut input = ActorTickInput {
        yaw: 135.0,
        body_yaw: 25.0,
        head_yaw: -40.0,
        ..ActorTickInput::default()
    };
    assert_eq!(read(&actor, &input, 0, "query.target_y_rotation"), 135.0);
    actor.yaw = -170.0;
    // The frame's interpolated sample owns the query, not the newest packet.
    input.yaw = 165.0;
    assert_eq!(read(&actor, &input, 0, "query.target_y_rotation"), 165.0);
}

#[test]
fn hud_pose_advances_crawling_animation_independently_of_first_person() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/assets/compiled");
    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!(
                "skipping HUD pose test: missing entity fixture directory {}",
                root.display()
            );
            return;
        }
        Err(error) => panic!("read entity fixture directory {}: {error}", root.display()),
    };
    let Some(path) = entries
        .map(|entry| entry.expect("read entity fixture entry").path())
        .find(|path| {
            path.extension()
                .is_some_and(|extension| extension == "mcbeent")
        })
    else {
        eprintln!(
            "skipping HUD pose test: missing entity fixture {}/*.mcbeent",
            root.display()
        );
        return;
    };
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!(
                "skipping HUD pose test: missing entity fixture {}",
                path.display()
            );
            return;
        }
        Err(error) => panic!("read entity fixture {}: {error}", path.display()),
    };
    let assets = Arc::new(RuntimeEntityAssets::decode(&bytes).unwrap());
    let mut actor = actor_with_metadata(HashMap::from([(
        crate::actor_store::EXTENDED_FLAGS_METADATA_KEY,
        ActorMetadataValue::FlagsExtended(
            1 << (crate::actor_store::ACTOR_FLAG_CRAWLING - u64::BITS),
        ),
    )]));
    actor.kind = ActorKind::Player {
        uuid: [0; 16],
        username: "Offline".into(),
    };
    let mut first = ActorAnimationStore::with_assets(Arc::clone(&assets));
    let mut third = ActorAnimationStore::with_assets(assets);
    first.insert(1, 0, &actor);
    third.insert(1, 0, &actor);
    let mut actors = HashMap::from([(actor.runtime_id, actor)]);
    let mut previous_hud = None;
    for tick in 0..4 {
        // The pinned crawl clip advances from modified_distance_moved, so this
        // fixture must move before comparing successive HUD keyframes.
        actors.get_mut(&1).unwrap().position[0] = tick as f32 * 0.2;
        first.advance_tick(&actors, None, Some(1), true, false, |_| ActorTickContext {
            is_local_first_person: true,
            ..Default::default()
        });
        third.advance_tick(&actors, None, Some(1), true, false, |_| {
            ActorTickContext::default()
        });
        let hud = first.ui_pose(1).expect("full body pose");
        assert_eq!(hud, third.ui_pose(1).unwrap(), "HUD tick {tick}");
        if let Some(previous) = previous_hud {
            assert_ne!(hud, previous, "the crawling keyframes must advance");
        }
        previous_hud = Some(hud.to_vec());
    }
    let hud = first.ui_pose(1).expect("full body pose");
    assert!(!hud.is_empty());
    assert_eq!(hud, third.ui_pose(1).unwrap());
    assert_ne!(hud, first.get(1).unwrap().current);
}
