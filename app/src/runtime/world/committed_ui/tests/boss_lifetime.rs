use super::*;

fn boss_actor(dimension: i32, unique_id: i64, runtime_id: u64) -> WorldEvent {
    WorldEvent::Actor(protocol::ActorEvent::Spawn(protocol::ActorSpawnEvent {
        dimension,
        unique_id,
        runtime_id,
        kind: protocol::ActorKind::Entity {
            identifier: "minecraft:ender_dragon".into(),
        },
        position: [0.0, 70.0, 0.0],
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
    }))
}

fn show_boss(unique_id: i64) -> WorldEvent {
    WorldEvent::Ui(UiEvent::Boss(protocol::BossEvent {
        target_entity_id: unique_id,
        action: protocol::BossAction::Show,
        title: "Dragon".into(),
        filtered_title: "Dragon".into(),
        progress: 1.0,
        style: protocol::BossStyle {
            color: protocol::BossColor::Purple,
            overlay: protocol::BossOverlay::Progress,
            darken_sky: None,
            create_world_fog: None,
        },
    }))
}

fn submit(app: &mut App, sequence: u64, event: WorldEvent) {
    app.world_mut()
        .resource_mut::<ClientWorld>()
        .stream
        .as_mut()
        .unwrap()
        .submit(sequence, event)
        .unwrap();
}

fn dimension(dimension: i32) -> WorldEvent {
    WorldEvent::ChangeDimension(protocol::ChangeDimensionEvent {
        dimension,
        position: [0.0, 70.0, 0.0],
        ..Default::default()
    })
}

#[test]
fn unresolved_boss_show_does_not_acknowledge_registration_before_actor_admission() {
    let (mut app, _) = fixture_app();
    let mut schedule = Schedule::default();
    schedule.add_systems(drain_committed_ui_before_authority);
    submit(&mut app, 1, dimension(2));
    submit(&mut app, 2, show_boss(-17));
    schedule.run(app.world_mut());
    {
        let mut ui = app.world_mut().resource_mut::<UiRuntime>();
        assert!(ui.boss_bars().stacked().is_empty());
        assert_eq!(
            ui.flush_boss_responses(|_| Ok(())),
            Ok(0),
            "an unresolved Show must leave the server subscription unacknowledged"
        );
    }

    submit(&mut app, 3, boss_actor(2, -17, 17));
    submit(&mut app, 4, show_boss(-17));
    schedule.run(app.world_mut());
    let mut ui = app.world_mut().resource_mut::<UiRuntime>();
    assert_eq!(ui.boss_bars().stacked()[0].target_entity_id, -17);
    assert_eq!(ui.flush_boss_responses(|_| Ok(())), Ok(1));
}

#[test]
fn committed_boss_bars_retire_only_when_their_actor_leaves_authority() {
    let (mut app, _) = fixture_app();
    let mut schedule = Schedule::default();
    schedule.add_systems(drain_committed_ui_before_authority);
    submit(&mut app, 1, dimension(2));
    submit(&mut app, 2, boss_actor(2, -17, 17));
    submit(&mut app, 3, boss_actor(2, -18, 18));
    submit(&mut app, 4, show_boss(-17));
    submit(&mut app, 5, show_boss(-18));
    schedule.run(app.world_mut());
    {
        let mut ui = app.world_mut().resource_mut::<UiRuntime>();
        assert_eq!(ui.boss_bars().stacked().len(), 2);
        assert_eq!(ui.flush_boss_responses(|_| Ok(())), Ok(2));
    }

    submit(
        &mut app,
        6,
        WorldEvent::Actor(protocol::ActorEvent::Remove(protocol::ActorRemoveEvent {
            dimension: 2,
            unique_id: -17,
        })),
    );
    schedule.run(app.world_mut());
    {
        let mut ui = app.world_mut().resource_mut::<UiRuntime>();
        assert_eq!(
            ui.boss_bars()
                .stacked()
                .iter()
                .map(|bar| bar.target_entity_id)
                .collect::<Vec<_>>(),
            [-18]
        );
        assert_eq!(ui.boss_bars().retained_text_bytes(), 12);
        assert_eq!(
            ui.flush_boss_responses(|_| panic!("local actor loss is not a wire Hide")),
            Ok(0)
        );
    }

    submit(&mut app, 7, dimension(0));
    schedule.run(app.world_mut());
    {
        let mut ui = app.world_mut().resource_mut::<UiRuntime>();
        assert!(
            ui.boss_bars().stacked().is_empty(),
            "the End actor no longer exists in the Overworld"
        );
        assert_eq!(ui.boss_bars().retained_text_bytes(), 0);
        assert_eq!(
            ui.flush_boss_responses(|_| panic!("dimension cleanup must stay local")),
            Ok(0)
        );
    }

    submit(&mut app, 8, boss_actor(0, -18, 18));
    submit(&mut app, 9, show_boss(-18));
    schedule.run(app.world_mut());
    let mut ui = app.world_mut().resource_mut::<UiRuntime>();
    assert_eq!(ui.boss_bars().stacked()[0].target_entity_id, -18);
    assert_eq!(ui.flush_boss_responses(|_| Ok(())), Ok(1));
    assert!(app.world().resource::<ClientWorld>().fatal_error.is_none());
}
