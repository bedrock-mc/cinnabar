use crate::actor_store::ActorStore;
use protocol::{ActorEvent, ActorSpawnEvent, ActorStatusEvent, ActorStatusKind};

/// Spawns a stationary rig with assets authored by the test suite.
fn store() -> ActorStore {
    let mut store = ActorStore::new_with_entity_assets(
        1,
        0,
        super::render_frame::tests::counting_random_assets(),
    );
    let actor = super::tests::actor_with_metadata(Default::default());
    store.apply(
        1,
        1,
        ActorEvent::Spawn(ActorSpawnEvent {
            dimension: 0,
            unique_id: actor.unique_id,
            runtime_id: actor.runtime_id,
            kind: actor.kind,
            position: actor.position,
            velocity: actor.velocity,
            pitch: actor.pitch,
            yaw: actor.yaw,
            head_yaw: actor.head_yaw,
            body_yaw: actor.body_yaw,
            held_item: Default::default(),
            metadata: Default::default(),
            attributes: Default::default(),
            properties: Default::default(),
            links: Default::default(),
        }),
    );
    store
}

/// Sends the same status packet path used for local and remote actors.
fn status(store: &mut ActorStore, sequence: u64, kind: ActorStatusKind) {
    store.apply(
        1,
        sequence,
        ActorEvent::Status(ActorStatusEvent {
            runtime_id: 1,
            kind,
            data: 0,
        }),
    );
}

#[test]
fn consecutive_hurt_events_each_accelerate_limb_swing() {
    for kind in [ActorStatusKind::Hurt, ActorStatusKind::HurtWithoutDamage] {
        let mut store = store();
        for hit in 0..3 {
            status(&mut store, hit + 2, kind);
            store.advance_interpolation_ticks(1);
            let motion = store.actor_rig(1).unwrap().java;
            assert!((motion.limb_amount[1] - 0.9).abs() < 1e-6, "hit {hit}");
            assert!((motion.limb_swing[1] - 0.9 * (hit + 1) as f32).abs() < 1e-6);
        }
        store.advance_interpolation_ticks(1);
        let motion = store.actor_rig(1).unwrap().java;
        assert!((motion.limb_amount[1] - 0.54).abs() < 1e-6);
        assert!((motion.limb_swing[1] - 3.24).abs() < 1e-6);
    }
}

#[test]
fn hurt_changes_the_rendered_amount_before_the_next_tick() {
    let mut store = store();
    status(&mut store, 2, ActorStatusKind::Hurt);
    let motion = store.actor_rig(1).unwrap().java;
    assert_eq!(motion.limb_amount, [0.0, 1.5]);
    assert_eq!(motion.limb_swing, [0.0; 2]);
    status(&mut store, 3, ActorStatusKind::Hurt);
    assert_eq!(store.actor_rig(1).unwrap().java, motion);
}

#[test]
fn death_without_a_hurt_event_does_not_accelerate_limb_swing() {
    let mut store = store();
    status(&mut store, 2, ActorStatusKind::Death);
    store.advance_interpolation_ticks(1);
    let motion = store.actor_rig(1).unwrap().java;
    assert_eq!(motion.limb_amount, [0.0; 2]);
    assert_eq!(motion.limb_swing, [0.0; 2]);
}
