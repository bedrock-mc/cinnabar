//! A radius reply updates play without being an additional spawn prerequisite.

use super::*;
use futures::FutureExt;

/// Returns an ordered world event around the late server radius clamp.
fn time(value: i32) -> Packet {
    Packet::new(SetTime {
        time: wire::ZigZag32(value),
    })
}

#[tokio::test]
async fn prepared_startup_does_not_wait_for_radius_but_keeps_spawn_and_registry_gates() {
    for withheld in [
        None,
        Some(packets::PacketId::PlayStatus),
        Some(packets::PacketId::ItemRegistry),
    ] {
        let (client, peer) = tokio::io::duplex(8192);
        let (read, write) = tokio::io::split(client);
        let (reader, frames, writer) =
            bridge::tasks::prepare_bounded_connection(read, write, bridge::MAX_FRAME_LEN, 4096);
        let writer = tokio::spawn(writer);
        let jobs = Rc::new(Cell::new(0));
        let preparation = LocalPreparation(Rc::clone(&jobs));
        let settings = LoginSettings {
            view_distance_chunks: 23,
            ..Default::default()
        };
        let [registry, radius, spawn] = <[Packet; 3]>::try_from(spawn_prerequisites()).unwrap();
        let (initial, held) = match withheld {
            Some(packets::PacketId::PlayStatus) => (vec![registry], Some(spawn)),
            Some(packets::PacketId::ItemRegistry) => (vec![spawn], Some(registry)),
            None => (vec![registry, spawn], None),
            _ => unreachable!("only startup facts are withheld"),
        };
        let mut join = Box::pin(LoginSequence::connect_prepared(
            SessionIo {
                reader,
                frames,
                timers: bridge::tasks::Timers::tokio(),
            },
            SessionLogin {
                display_name: "Fixture",
                client_random_id: ClientRandomId::new(-7).unwrap(),
                cache: None,
                skin: None,
                settings: &settings,
                tap: None,
                relay: None,
                target: Some(bridge::ConnectTarget::RakNet(
                    "game.example.test:19132".into(),
                )),
            },
            &preparation,
            keep_prepared,
        ));
        let core = async {
            let mut core = FakeCore { stream: peer };
            core.receive().await;
            core.send(handoff_frame(
                serde_json::json!({"identity":identity("Fixture"),"client_cache":false,"packs_required":false,"packs":[]}),
                &[start_game()],
            )).await;
            let requests = core.receive_packets().await;
            assert!(
                matches!(&*requests[0].data, packets::Packet::RequestChunkRadius(request)
                if request.chunk_radius.0 == i32::from(settings.view_distance_chunks))
            );
            core.send(batch_frame(&initial)).await;
            core
        };
        // The in-memory core has completed every write before this returns, so the next
        // single poll observes all available prerequisites without a timer or scheduler delay.
        let mut core = tokio::select! {
            _ = &mut join => panic!("startup cannot finish before the core delivers its facts"),
            core = core => core,
        };
        let early = join.as_mut().now_or_never();
        let joined = if let Some(held) = held {
            assert!(
                early.is_none(),
                "missing {withheld:?} must still hold startup"
            );
            if withheld == Some(packets::PacketId::PlayStatus) {
                assert_eq!(
                    jobs.get(),
                    2,
                    "preparation overlaps the missing PlayerSpawn"
                );
            }
            core.send(batch_frame(&[held])).await;
            join.as_mut().now_or_never()
        } else {
            early
        };
        let (mut session, (_, data)) = joined
            .expect("startup must complete while the radius reply is withheld")
            .expect("all other startup prerequisites are valid");
        drop(join);
        assert_eq!(jobs.get(), 2);
        assert_eq!(data.world.local_player_runtime_id, RUNTIME_ID);
        core.send(batch_frame(&[time(1), radius, time(2)])).await;
        assert!(
            matches!(session.recv_world_event(0).await.unwrap(), crate::WorldEvent::SetTime(event) if event.time == 1)
        );
        assert!(matches!(
            session.recv_world_event(0).await.unwrap(),
            crate::WorldEvent::ChunkRadiusUpdated(8)
        ));
        assert!(
            matches!(session.recv_world_event(0).await.unwrap(), crate::WorldEvent::SetTime(event) if event.time == 2)
        );
        session.finish_loading().await.unwrap();
        let completion = core.receive_packets().await;
        assert_eq!(completion.len(), 2);
        assert!(
            matches!(&*completion[1].data, packets::Packet::SetLocalPlayerAsInitialized(initialized) if initialized.player_id.0 == RUNTIME_ID)
        );
        drop(session);
        writer.await.unwrap();
    }
}
