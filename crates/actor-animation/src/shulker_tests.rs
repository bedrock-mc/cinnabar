use super::super::{tests::actor_with_metadata, tick};
use super::*;

const PEEK_KEY: u32 = ActorDataKey::PeekID.wire_value();
const FACE_KEY: u32 = ActorDataKey::AttachFace.wire_value();

fn shulker(metadata: HashMap<u32, ActorMetadataValue>) -> TestActor {
    let mut actor = actor_with_metadata(metadata);
    actor.kind = ActorKind::Entity {
        identifier: NativeEntityClass::Shulker.identifier().unwrap().into(),
    };
    actor
}

fn engine() -> EngineSlots {
    EngineSlots {
        shulker_peek_amount: Some(0),
        shulker_facing_direction: Some(1),
        ..EngineSlots::default()
    }
}

/// Runs one engine-variable refresh with `peek` as the shulker's lid-open fraction.
fn publish(actor: &TestActor, peek: f32, riding: bool) -> [Option<f32>; 2] {
    let mut variables = MolangVariables::slots(2);
    let mut motion = MotionState::default();
    motion.shulker_peek = peek;
    tick::apply_engine_variables(
        &engine(),
        &mut variables,
        actor,
        &ActorTickContext {
            is_riding: riding,
            ..Default::default()
        },
        &ActorTickInput::default(),
        &motion,
    );
    [variables.number_at(0), variables.number_at(1)]
}

#[test]
fn peek_opens_and_closes_by_native_steps_toward_the_synced_target() {
    let mut actor = shulker(HashMap::from([(PEEK_KEY, ActorMetadataValue::Int(30))]));
    let mut peek = 0.0;
    let mut opened = Vec::new();
    for _ in 0..8 {
        advance_peek(&mut peek, &actor);
        opened.push(peek);
    }
    let expected = [0.05, 0.1, 0.15, 0.2, 0.25, 0.3, 0.3, 0.3];
    for (got, want) in opened.iter().zip(expected) {
        assert!((got - want).abs() < 1e-6, "{opened:?}");
    }
    actor.metadata.insert(PEEK_KEY, ActorMetadataValue::Int(0));
    for _ in 0..5 {
        advance_peek(&mut peek, &actor);
    }
    assert!((peek - 0.05).abs() < 1e-6);
    advance_peek(&mut peek, &actor);
    assert_eq!(peek, 0.0);
}

#[test]
fn peek_ignores_other_data_types_and_other_actors() {
    let mut peek = 0.5;
    let actor = shulker(HashMap::from([(PEEK_KEY, ActorMetadataValue::Byte(100))]));
    advance_peek(&mut peek, &actor);
    assert!((peek - 0.45).abs() < 1e-6);
    let mut other = actor_with_metadata(HashMap::from([(PEEK_KEY, ActorMetadataValue::Int(100))]));
    other.kind = ActorKind::Entity {
        identifier: "minecraft:pig".into(),
    };
    advance_peek(&mut peek, &other);
    assert!((peek - 0.45).abs() < 1e-6);
}

#[test]
fn peek_above_one_snaps_to_the_closing_bound() {
    let mut peek = 2.0;
    let actor = shulker(HashMap::from([(PEEK_KEY, ActorMetadataValue::Int(0))]));
    advance_peek(&mut peek, &actor);
    assert_eq!(peek, 1.0);
}

#[test]
fn shulker_variables_ease_the_lid_and_read_the_attach_face() {
    let actor = shulker(HashMap::from([(FACE_KEY, ActorMetadataValue::Byte(3))]));
    let [closed, facing] = publish(&actor, 0.0, false);
    assert!(closed.unwrap().abs() < 1e-6);
    assert_eq!(facing, Some(3.0));
    let [open, _] = publish(&actor, 1.0, false);
    assert!((open.unwrap() - 1.0).abs() < 1e-6);
    let [half, _] = publish(&actor, 0.5, false);
    assert!((half.unwrap() - 0.5).abs() < 1e-3);
    assert_eq!(publish(&actor, 0.0, true)[1], Some(0.0));
    let int_face = shulker(HashMap::from([(FACE_KEY, ActorMetadataValue::Int(3))]));
    assert_eq!(publish(&int_face, 0.0, false)[1], Some(0.0));
}

#[test]
fn shulker_variables_leave_other_actors_untouched() {
    let mut actor = actor_with_metadata(HashMap::from([(FACE_KEY, ActorMetadataValue::Byte(3))]));
    actor.kind = ActorKind::Entity {
        identifier: "minecraft:pig".into(),
    };
    assert_eq!(publish(&actor, 1.0, false), [None, None]);
}

/// The shulker post-AI step zeroes body yaw every tick, so the model never turns with the
/// synced yaw; only its head follows the target.
#[test]
fn shulker_body_never_turns_with_its_yaw() {
    let identifier = NativeEntityClass::Shulker.identifier().unwrap();
    let mut actor = shulker(HashMap::from([(PEEK_KEY, ActorMetadataValue::Int(100))]));
    actor.yaw = 45.0;
    actor.head_yaw = 45.0;
    let mut actors = HashMap::from([(actor.runtime_id, actor)]);
    let mut store = ActorAnimationStore::with_assets(
        crate::test_support::counting_random_assets_for(identifier),
    );
    store.insert(1, 0, &actors[&1]);
    for yaw in [45.0, 120.0, -60.0] {
        let actor = actors.get_mut(&1).unwrap();
        actor.yaw = yaw;
        actor.head_yaw = yaw;
        store.advance_tick(&actors, None, None, true, true, |_| {
            ActorTickContext::default()
        });
        assert_eq!(store.get(1).unwrap().body_yaw, 0.0, "yaw {yaw}");
    }
    let state = store.rigs.values().next().unwrap();
    assert!((state.motion.shulker_peek - 0.15).abs() < 1e-6);
}
