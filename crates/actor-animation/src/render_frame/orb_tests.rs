use super::*;

#[test]
fn installed_xp_colour_uses_current_actor_age_when_world_animation_budget_is_exhausted() {
    let Some(assets) = crate::test_support::installed_entity_assets("XP animation") else {
        return;
    };
    let mut actor = super::super::tests::actor_with_metadata(HashMap::new());
    actor.kind = protocol::ActorKind::Entity {
        identifier: "minecraft:xp_orb".into(),
    };
    actor.runtime_id = u64::MAX;
    let mut actors = HashMap::from([(actor.runtime_id, actor.clone())]);
    let mut starved = ActorAnimationStore::with_assets(Arc::clone(&assets));
    let mut visible = ActorAnimationStore::with_assets(assets);
    for animation in [&mut starved, &mut visible] {
        animation.insert(1, 0, &actor);
        for _ in 0..2 {
            animation.advance_tick(&actors, None, None, true, true, |_| {
                ActorTickContext::default()
            });
        }
    }
    let before_ops = visible.stats().evaluated_molang_ops;
    visible.advance_tick(&actors, None, None, true, true, |_| {
        ActorTickContext::default()
    });
    let actor_ops = visible.stats().evaluated_molang_ops - before_ops;
    assert!(actor_ops > 0);
    let pressure_actors = MAX_MOLANG_OPS_PER_WORLD_TICK / actor_ops as usize + 2;
    assert!(pressure_actors < MAX_ANIMATED_ACTORS);
    for runtime_id in 1..=pressure_actors as u64 {
        let mut pressure = actor.clone();
        pressure.runtime_id = runtime_id;
        pressure.unique_id = runtime_id as i64;
        starved.insert(1, 0, &pressure);
        actors.insert(runtime_id, pressure);
    }
    let held = starved.get(actor.runtime_id).unwrap();
    let held_tick = held.completed_tick;
    let held_pose = held.current.to_vec();
    let held_layers = held.render.to_vec();
    starved.advance_tick(&actors, None, None, true, true, |_| {
        ActorTickContext::default()
    });
    assert!(starved.stats().world_budget_exhaustions > 0);
    assert_eq!(
        starved.get(actor.runtime_id).unwrap().completed_tick,
        held_tick
    );
    let stats = starved.stats();
    for alpha in [0.25, 0.75] {
        let mut expected_ops = MAX_MOLANG_OPS_PER_RENDER_FRAME;
        let expected = visible
            .render_layers(&actor, alpha, [0.0; 2], [0.0; 3], &mut expected_ops, false)
            .unwrap();
        let mut actual_ops = MAX_MOLANG_OPS_PER_RENDER_FRAME;
        let actual = starved
            .render_layers(&actor, alpha, [0.0; 2], [0.0; 3], &mut actual_ops, false)
            .unwrap();
        assert_eq!(
            actual.render[0].overlay, expected.render[0].overlay,
            "rendered XP colour must use live actor age while the world budget holds its pose"
        );
    }
    assert_eq!(starved.get(actor.runtime_id).unwrap().current, held_pose);
    assert_eq!(starved.get(actor.runtime_id).unwrap().render, held_layers);
    assert_eq!(starved.stats(), stats);
}
