use super::*;
use bevy::prelude::{App, Update};
use client_ui::ui_runtime::SequencedUiEvent;
use protocol::{BossAction, BossColor, BossEvent, BossOverlay, BossStyle, UiEvent};

#[test]
fn boss_reply_drain_observes_the_transport_session_fence_during_credits() {
    let mut player = crate::player_runtime::PlayerRuntime::new(1);
    let mut runtime = UiRuntime::new(1);
    runtime
        .apply(
            &mut player,
            SequencedUiEvent {
                session_id: 1,
                fifo_sequence: 1,
                local_millis: 0,
                server_tick: None,
                event: UiEvent::Boss(BossEvent {
                    target_entity_id: -17,
                    action: BossAction::Show,
                    title: "Dragon".into(),
                    filtered_title: String::new().into(),
                    progress: 1.0,
                    style: BossStyle {
                        color: BossColor::Purple,
                        overlay: BossOverlay::Progress,
                        darken_sky: None,
                        create_world_fog: None,
                    },
                }),
            },
        )
        .unwrap();
    assert!(runtime.credits_mut().open(41, 2, 0));
    assert_eq!(
        runtime.flush_boss_responses(|_| Err(FormTransportError::Full)),
        Err(FormTransportError::Full)
    );
    let (network, _queue_guard) = NetworkHandle::with_command_capacity(2);
    let mut app = App::new();
    app.insert_resource(player)
        .insert_resource(runtime)
        .insert_resource(network)
        .add_systems(Update, flush_server_form_network);
    app.update();
    let mut runtime = app.world_mut().resource_mut::<UiRuntime>();
    assert!(runtime.credits().owns_input());
    assert_eq!(
        runtime.flush_boss_responses(|_| panic!("closed session must retire its boss reply")),
        Ok(0)
    );
}
