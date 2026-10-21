use super::*;

#[test]
fn unsupported_npc_dismissal_consumes_input_without_a_packet() {
    for action in [KeyCode::Escape, KeyCode::Enter] {
        let mut player = client_state::PlayerRuntime(player_state::PlayerState::new(1));
        let mut event = retained(protocol::NPC_DIALOGUE_FORM_ID, 1);
        let protocol::UiEvent::Form(form) = &mut event.event else {
            unreachable!();
        };
        form.kind = protocol::FormKind::NpcDialogue;
        form.model = protocol::ServerFormModel::Unsupported(protocol::UnsupportedForm::Controls);
        let (mut app, window) = app_for_event(&mut player, event);
        press(&mut app, window, action);
        press(&mut app, window, KeyCode::KeyW);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Right);
        app.world_mut()
            .resource_mut::<AccumulatedMouseMotion>()
            .delta = Vec2::new(4.0, 2.0);

        app.update();

        assert!(
            !app.world().resource::<MenuRuntime>().is_visible(),
            "the dismissal press must not open pause"
        );
        let runtime = app.world().resource::<UiRuntimeResource>();
        assert!(runtime.server_forms().active().is_none());
        assert!(runtime.server_forms().owns_input());
        assert!(!runtime.chat_focused() && !runtime.inventory_open());
        let keys = app.world().resource::<ButtonInput<KeyCode>>();
        assert!(keys.get_pressed().next().is_none());
        assert!(!keys.just_pressed(action));
        let mouse = app.world().resource::<ButtonInput<MouseButton>>();
        assert!(mouse.get_pressed().next().is_none());
        assert!(!mouse.just_pressed(MouseButton::Right));
        assert_eq!(
            app.world().resource::<AccumulatedMouseMotion>().delta,
            Vec2::ZERO
        );
        assert!(
            !flush_form_response(
                &mut app.world_mut().resource_mut::<UiRuntimeResource>(),
                |_| false,
                |_| panic!("unsupported NPC dismissal must not send a response"),
            )
            .unwrap()
        );
        assert!(
            !app.world()
                .resource::<UiRuntimeResource>()
                .server_forms()
                .owns_input()
        );

        app.update();
        assert!(!app.world().resource::<MenuRuntime>().is_visible());
        press(&mut app, window, KeyCode::Escape);
        app.update();
        assert!(
            app.world().resource::<MenuRuntime>().is_visible(),
            "a new press resumes normal pause controls"
        );
    }
}

#[test]
fn unsupported_npc_dismissal_discards_later_shortcuts() {
    for shortcut in [KeyCode::KeyT, KeyCode::KeyE] {
        let mut player = client_state::PlayerRuntime(player_state::PlayerState::new(1));
        let mut event = retained(protocol::NPC_DIALOGUE_FORM_ID, 1);
        let protocol::UiEvent::Form(form) = &mut event.event else {
            unreachable!();
        };
        form.kind = protocol::FormKind::NpcDialogue;
        form.model = protocol::ServerFormModel::Unsupported(protocol::UnsupportedForm::Controls);
        let (mut app, window) = app_for_event(&mut player, event);
        press(&mut app, window, KeyCode::Escape);
        press(&mut app, window, shortcut);

        app.update();

        let runtime = app.world().resource::<UiRuntimeResource>();
        assert!(!runtime.chat_focused(), "the form owned the chat shortcut");
        assert!(
            !runtime.inventory_open(),
            "the form owned the inventory shortcut"
        );
    }
}
