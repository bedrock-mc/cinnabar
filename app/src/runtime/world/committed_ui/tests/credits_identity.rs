use super::*;

#[test]
fn committed_actor_names_preserve_login_chat_identity_and_credits_name() {
    let (mut app, _) = fixture_app();
    app.world_mut()
        .resource_mut::<UiRuntimeResource>()
        .set_chat_identity("Account Player".into(), "2535".into());
    let mut feed = client_world::LocalPlayerFeed {
        game_mode: None,
        uuid: [7; 16],
        prefer_client_skin: false,
        username: "CurrentLocalPlayer".into(),
        skin: protocol::PlayerSkin::Standard(render_api::StandardSkin {
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
        held_network_ids: [0; 2],
        held_variants: Default::default(),
        main_hand_metadata: 0,
        main_hand_slot: 0,
        main_hand_stack_id: None,
        bedrock_swing_ticks: actor_animation::ACTOR_SWING_TICKS,
        java_swing_ticks: actor_animation::ACTOR_SWING_TICKS,
        flying: false,
        gliding: false,
        spinning: false,
        fall_fly_ticks: 0,
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
            app.world()
                .resource::<UiRuntimeResource>()
                .credits_player_name(),
            name,
            "the production drain must refresh identity even without a UI packet"
        );
        let mut ui = app.world_mut().resource_mut::<UiRuntimeResource>();
        ui.queue_chat_message("hello", 0).unwrap();
        let (sequence, packet) = ui.next_chat_packet().unwrap().unwrap();
        assert_eq!(
            protocol::encode(&packet).unwrap(),
            protocol::encode(
                &protocol::chat_text_packet("Account Player", "2535", "hello").unwrap()
            )
            .unwrap(),
            "actor display names must not replace the login chat identity"
        );
        assert!(ui.confirm_chat_send(sequence));
    }
}
