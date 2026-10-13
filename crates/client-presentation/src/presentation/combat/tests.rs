//! Original synthetic actors for combat presentation contracts.
use super::*;
use chunk_pipeline::WorldStream;
use protocol::{ActorEvent, ActorSpawnEvent, ActorStatusEvent, ActorStatusKind, WorldEvent};
use std::sync::Arc;

/// Creates one ordinary player without installed artwork or a live account.
pub(crate) fn player_stream() -> WorldStream {
    let mut stream = WorldStream::new(protocol::WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 2,
        local_player_unique_id: 2,
        player_position: [0.0, 64.0, 0.0],
        world_spawn_position: [0; 3],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    });
    stream
        .submit(
            1,
            WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                dimension: 0,
                unique_id: 2,
                runtime_id: 2,
                kind: protocol::ActorKind::Player {
                    uuid: [1; 16],
                    username: "fixture".into(),
                },
                position: [0.0, 64.0, 0.0],
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
            })),
        )
        .unwrap();
    stream
}

/// Delivers one accepted actor event in its requested sequence.
pub(crate) fn event(stream: &mut WorldStream, sequence: u64, kind: ActorStatusKind) {
    stream
        .submit(
            sequence,
            WorldEvent::Actor(ActorEvent::Status(ActorStatusEvent {
                runtime_id: 2,
                kind,
                data: 0,
            })),
        )
        .unwrap();
}

#[test]
fn overlays_remain_flat_for_positive_ticks_and_restart_on_real_damage() {
    let mut stream = player_stream();
    for profile in [CombatVisuals::Legacy112, CombatVisuals::Modern] {
        event(&mut stream, 2, ActorStatusKind::Hurt);
        let expected = overlay(stream.authority().actor(2).unwrap(), profile);
        assert_ne!(expected, 0);
        for _ in 0..BEDROCK_112.hurt_ticks {
            assert_eq!(
                overlay(stream.authority().actor(2).unwrap(), profile),
                expected
            );
            stream.advance_actor_interpolation_frame(1);
        }
        assert_eq!(overlay(stream.authority().actor(2).unwrap(), profile), 0);
        event(&mut stream, 3, ActorStatusKind::Hurt);
        stream.advance_actor_interpolation_frame(7);
        event(&mut stream, 4, ActorStatusKind::Hurt);
        let actor = stream.authority().actor(2).unwrap();
        assert_eq!(actor.status.hurt_time, BEDROCK_112.hurt_ticks);
        assert_eq!(overlay(actor, profile), expected);
        event(&mut stream, 5, ActorStatusKind::HurtWithoutDamage);
        assert_eq!(overlay(stream.authority().actor(2).unwrap(), profile), 0);
        event(&mut stream, 6, ActorStatusKind::Death);
        assert_eq!(
            overlay(stream.authority().actor(2).unwrap(), profile),
            expected
        );
        event(&mut stream, 7, ActorStatusKind::SpawnAlive);
        assert_eq!(overlay(stream.authority().actor(2).unwrap(), profile), 0);
        // Start a fresh session so the next profile's events are independent.
        stream = player_stream();
    }
    let mut stream = player_stream();
    event(&mut stream, 2, ActorStatusKind::Hurt);
    let actor = stream.authority().actor(2).unwrap();
    assert_eq!(
        overlay(actor, CombatVisuals::Legacy112),
        pack_overlay_rgba8(BEDROCK_112.hurt_overlay)
    );
    assert_eq!(
        overlay(actor, CombatVisuals::Modern),
        pack_overlay_rgba8([1.0, 0.0, 0.0, client_world::HURT_OVERLAY_ALPHA])
    );
}
