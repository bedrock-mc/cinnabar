//! Installed XP orb colour sampled between and after culled ticks.
use std::sync::Arc;

use crate::actor_store::ActorStore;
use actor_animation::{ActorAnimationView, test_support};

fn installed_orb() -> Option<ActorStore> {
    let assets = test_support::installed_entity_assets("XP animation")?;
    let mut store = ActorStore::new_with_entity_assets(1, 0, assets);
    store.apply(
        1,
        1,
        protocol::ActorEvent::Spawn(protocol::ActorSpawnEvent {
            dimension: 0,
            unique_id: 1,
            runtime_id: 1,
            kind: protocol::ActorKind::Entity {
                identifier: "minecraft:xp_orb".into(),
            },
            position: [0.0; 3],
            velocity: [0.0; 3],
            pitch: 0.0,
            yaw: 0.0,
            head_yaw: 0.0,
            body_yaw: 0.0,
            held_item: Default::default(),
            metadata: Arc::from([]),
            attributes: Arc::from([]),
            properties: Arc::from([]),
            links: Arc::from([]),
        }),
    );
    store.advance_interpolation_ticks(2);
    Some(store)
}

#[test]
fn installed_xp_colour_advances_between_ticks_without_mutating_actor_or_rig_state() {
    let Some(mut store) = installed_orb() else {
        return;
    };
    let rig = store.actor_rig(1).unwrap();
    let pose = rig.current.to_vec();
    let completed_tick = rig.completed_tick;
    let tick_layers = rig.render.to_vec();
    let actor = store.get(1).unwrap().clone();
    let stats = store.animation_stats();
    let early = store.render_frame(0.25).layers(1).unwrap().into_owned();
    let late = store.render_frame(0.75).layers(1).unwrap().into_owned();
    assert!(
        (late[0].overlay[0] - early[0].overlay[0]).abs() > 0.001,
        "the authored XP lifetime overlay must animate within a single completed tick: early {:?}, late {:?}",
        early[0].overlay,
        late[0].overlay
    );
    assert_eq!(early[0].overlay[1], 1.0);
    assert_eq!(late[0].overlay[3], 0.5);
    assert_eq!(early, store.render_frame(0.25).layers(1).unwrap().as_ref());
    assert_eq!(store.actor_rig(1).unwrap().completed_tick, completed_tick);
    assert_eq!(store.actor_rig(1).unwrap().current, pose);
    assert_eq!(store.actor_rig(1).unwrap().render, tick_layers);
    assert_eq!(store.get(1).unwrap(), &actor);
    assert_eq!(store.animation_stats(), stats);
    store.advance_interpolation_ticks(1);
    assert_ne!(
        store.actor_rig(1).unwrap().render[0].overlay,
        tick_layers[0].overlay
    );
}

#[test]
fn installed_xp_colour_uses_current_actor_age_after_culled_ticks_without_a_new_tick() {
    let (Some(mut culled), Some(mut visible)) = (installed_orb(), installed_orb()) else {
        return;
    };
    culled.set_animation_view(Some(ActorAnimationView {
        planes: [[0.0, 0.0, 0.0, -1.0]; 6],
        camera: [0.0; 3],
        player_distance: 100.0,
        entity_radius: 100.0,
    }));
    culled.advance_interpolation_ticks(7);
    visible.advance_interpolation_ticks(7);
    culled.set_animation_view(None);
    let rig = culled.actor_rig(1).unwrap();
    let completed_tick = rig.completed_tick;
    let pose = rig.current.to_vec();
    let tick_layers = rig.render.to_vec();
    let actor = culled.get(1).unwrap().clone();
    let stats = culled.animation_stats();
    for alpha in [0.0, 0.25, 0.75] {
        let expected = visible.render_frame(alpha).layers(1).unwrap().into_owned();
        let actual = culled.render_frame(alpha).layers(1).unwrap().into_owned();
        assert_eq!(
            actual[0].overlay, expected[0].overlay,
            "rendered XP colour must follow the same actor age after culled ticks: frame fraction {alpha}"
        );
    }
    assert_eq!(culled.actor_rig(1).unwrap().completed_tick, completed_tick);
    assert_eq!(culled.actor_rig(1).unwrap().current, pose);
    assert_eq!(culled.actor_rig(1).unwrap().render, tick_layers);
    assert_eq!(culled.get(1).unwrap(), &actor);
    assert_eq!(culled.animation_stats(), stats);
}
