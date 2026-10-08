//! Server death reasons survive either packet order and retire on recovery.

use super::*;

/// Builds a reason without adding a chat broadcast.
fn reason(key: &str, parameters: &[&str]) -> UiEvent {
    UiEvent::DeathInfo(protocol::DeathInfoEvent {
        message: Arc::from(key),
        parameters: parameters.iter().map(|value| Arc::from(*value)).collect(),
    })
}

#[test]
fn death_rules_hide_without_discarding_reasons_and_reset_per_session() {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    runtime
        .apply(&mut player, envelope(1, 1, reason("server reason", &[])))
        .unwrap();
    runtime
        .apply(
            &mut player,
            envelope(
                1,
                2,
                UiEvent::GameRules {
                    hud: Default::default(),
                    death: protocol::DeathRules {
                        show_messages: Some(false),
                        immediate_respawn: Some(true),
                    },
                },
            ),
        )
        .unwrap();
    assert_eq!(runtime.death_reason(), "");
    assert!(runtime.immediate_respawn());
    runtime
        .apply(
            &mut player,
            envelope(
                1,
                3,
                UiEvent::GameRules {
                    hud: Default::default(),
                    death: protocol::DeathRules {
                        show_messages: Some(true),
                        immediate_respawn: None,
                    },
                },
            ),
        )
        .unwrap();
    assert_eq!(runtime.death_reason(), "server reason");
    assert!(runtime.immediate_respawn());
    runtime.begin_session(2);
    assert_eq!(runtime.death_reason(), "");
    assert!(!runtime.immediate_respawn());
}

#[test]
fn death_reason_localizes_before_or_after_zero_health_without_chat() {
    let entries = [
        ("death.attack.mob", "%1$s was slain by %2$s"),
        ("entity.zombie.name", "Zombie"),
    ]
    .map(|(key, value)| assets::LangEntry {
        key: key.into(),
        value: value.into(),
    });
    let bytes = assets::encode_lang_catalog([9; 32], [10; 32], &entries).unwrap();
    for reason_first in [false, true] {
        let mut player = player_state::PlayerState::new(1);
        let mut runtime = UiRuntime::new(1);
        runtime.set_lang_catalog(Arc::new(
            assets::RuntimeLangCatalog::decode(&bytes).unwrap(),
        ));
        let death = reason("death.attack.mob", &["Alex", "%entity.zombie.name"]);
        let health = UiEvent::Hud(HudEvent::Health { health: 0 });
        let events = if reason_first {
            [death, health]
        } else {
            [health, death]
        };
        for (index, event) in events.into_iter().enumerate() {
            runtime
                .apply(&mut player, envelope(1, index as u64 + 1, event))
                .unwrap();
        }
        assert_eq!(runtime.death_reason(), "Alex was slain by Zombie");
        assert!(runtime.chat().messages().is_empty());
        runtime
            .apply(&mut player, envelope(1, 3, reason("custom.reason", &[])))
            .unwrap();
        assert_eq!(runtime.death_reason(), "custom.reason");
        runtime
            .apply(
                &mut player,
                envelope(1, 4, UiEvent::Hud(HudEvent::Health { health: 20 })),
            )
            .unwrap();
        assert_eq!(runtime.death_reason(), "");
        runtime
            .apply(
                &mut player,
                envelope(1, 5, UiEvent::Hud(HudEvent::Health { health: 0 })),
            )
            .unwrap();
        assert_eq!(runtime.death_reason(), "");
        runtime
            .apply(&mut player, envelope(1, 6, reason("new.reason", &[])))
            .unwrap();
        runtime.begin_session(2);
        assert_eq!(runtime.death_reason(), "");
    }
}

#[test]
fn death_reason_clears_after_authoritative_health_attribute_recovery() {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    runtime
        .apply(
            &mut player,
            envelope(1, 1, UiEvent::Hud(HudEvent::Health { health: 0 })),
        )
        .unwrap();
    runtime
        .apply(&mut player, envelope(1, 2, reason("custom.reason", &[])))
        .unwrap();
    runtime
        .apply_local_attributes(
            &mut player,
            SequencedLocalAttributes {
                session_id: 1,
                fifo_sequence: 3,
                local_millis: 30,
                server_tick: 1,
                attributes: vec![protocol::ActorAttribute {
                    name: Arc::from("minecraft:health"),
                    min: 0.0,
                    max: 20.0,
                    current: 20.0,
                    default: Some(20.0),
                    modifiers: Arc::from([]),
                }]
                .into(),
            },
        )
        .unwrap();
    assert_eq!(runtime.death_reason(), "");
}
