use super::*;

#[test]
fn committed_ui_uses_the_current_local_players_name_for_credits() {
    let (mut app, _) = fixture_app();
    let mut feed = client_world::LocalPlayerFeed {
        uuid: [7; 16],
        username: "CurrentLocalPlayer".into(),
        skin: protocol::PlayerSkin::Standard(protocol::StandardSkin {
            geometry: None,
            cape: None,
            width: 64,
            height: 64,
            rgba8: vec![255; 64 * 64 * 4].into(),
        }),
        position: [0.0, 70.0, 0.0],
        velocity: [0.0; 3],
        on_ground: true,
        yaw: 0.0,
        head_yaw: 0.0,
        pitch: 0.0,
        main_hand: None,
        off_hand: None,
        teleported: false,
        first_person: true,
        view_bobbing: true,
        sneaking: false,
        sprinting: false,
        item_use: Default::default(),
    };
    let mut schedule = Schedule::default();
    schedule.add_systems(drain_committed_ui_before_authority);
    for name in ["CurrentLocalPlayer", "ChangedLocalPlayer"] {
        feed.username = name.into();
        feed.uuid[0] += 1;
        app.world_mut()
            .resource_mut::<ClientWorld>()
            .stream
            .as_mut()
            .unwrap()
            .sync_local_player_pose(&feed);
        schedule.run(app.world_mut());
        assert_eq!(
            app.world().resource::<UiRuntime>().credits_player_name(),
            name,
            "the production drain must refresh identity even without a UI packet"
        );
    }
}
