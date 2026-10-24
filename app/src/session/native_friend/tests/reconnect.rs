//! One physical service interruption while the original signed game session remains owned.

use super::*;

/// Drives the same original application frames until real service rebind or original worker failure.
async fn rebound(
    original: &mut Original,
    acknowledged: impl std::future::Future<Output = bool>,
    deadline: Instant,
) -> bool {
    tokio::pin!(acknowledged);
    tokio::time::timeout_at(deadline.into(), async {
        loop {
            original.app.update();
            let event = original
                .app
                .world_mut()
                .resource_mut::<NetworkHandle<PackApplication>>()
                .control_events_mut()
                .try_recv();
            match event {
                Ok(client_session::NetworkControlEvent::Failed { .. }) => return false,
                Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => return false,
                _ => {}
            }
            tokio::select! {
                result = &mut acknowledged => return result,
                _ = tokio::task::yield_now() => {}
            }
        }
    })
    .await
    .unwrap_or(false)
}

/// Keeps the same opaque intent, initialized game peer and one joined member through RTA replacement.
#[test]
fn controlled_native_friend_rta_reconnect_preserves_original_play_and_single_membership() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let outcomes = runtime.block_on(async {
        let mut outcomes = Vec::new();
        for legacy in [false, true] {
            let deadline = Instant::now() + Duration::from_secs(30);
            let mut original = Original::open(legacy, deadline).await;
            let mut recovery = original.service.hold_rta_reconnect();
            let host_xuid = original
                .app
                .world()
                .resource::<MenuRuntime>()
                .view()
                .friends[0]
                .xuid
                .clone();
            let queued = original.play();
            assert!(
                queued,
                "original qualified row must dispatch before interruption"
            );
            let host = original.service.listen(&host_xuid, deadline).await.unwrap();
            let (initialized, observed) = tokio::sync::oneshot::channel();
            let (marker, received_marker) = tokio::sync::oneshot::channel();
            let peer = tokio::spawn(host.serve(initialized, marker, deadline));
            original
                .service
                .joined()
                .await
                .expect("actual original member response");
            original.service.admit();
            let bootstrap = original.bootstrap(deadline).await;
            let initialized = tokio::time::timeout_at(deadline.into(), observed)
                .await
                .is_ok_and(|result| result.is_ok());
            assert!(
                bootstrap && initialized,
                "original actual signed initialization must precede RTA interruption"
            );
            recovery.disconnect();
            let recovered = rebound(&mut original, recovery.rebound(), deadline).await;
            let marker_sent = if recovered {
                let network = original
                    .app
                    .world()
                    .resource::<NetworkHandle<PackApplication>>();
                let sent = network
                    .send_packet(protocol::request_chunk_radius_packet(
                        protocol::test_support::friend_peer::MARKER_RADIUS,
                        protocol::test_support::friend_peer::MARKER_RADIUS,
                    ))
                    .is_ok();
                network.flush_frame();
                sent
            } else {
                false
            };
            let marker_received = if marker_sent {
                tokio::time::timeout_at(deadline.into(), received_marker)
                    .await
                    .is_ok_and(|result| result.is_ok())
            } else {
                false
            };
            original.disconnect();
            let retired = original.retired(deadline).await;
            let peer = peer.await.unwrap();
            recovery.stop();
            let (facts, joined) = original.finish(retired).await;
            let observation = recovery.observation();
            let facts = facts.expect("original service cleanup must join before recovery oracle");
            assert!(
                retired && joined && peer.retired.is_ok(),
                "original worker, host and service cleanup"
            );
            assert_eq!((facts.joins, facts.leaves), (1, 1));
            assert!(
                facts.nonce_published
                    && facts.minted
                    && facts.signaling_retired
                    && (facts.rta_retired
                        || (observation.upgrades == 1 && observation.first_closed))
            );
            assert!(
                observation.first_retired,
                "original interrupted RTA stream must physically retire"
            );
            outcomes.push((
                recovered,
                marker_received,
                peer.play.map_err(|error| error.kind()),
                observation,
            ));
        }
        outcomes
    });
    // Both original dialects' service/game/worker owners have joined before the capability oracle.
    assert!(
        outcomes
            .iter()
            .all(|(recovered, marker, play, observed)| *recovered
                && *marker
                && matches!(play, Ok(true))
                && observed.upgrades == 2
                && observed.acknowledgments == 2
                && observed.rebinds == 1
                && observed.synchronized),
        "original acknowledged RTA membership did not recover alongside the same live game: {outcomes:?}"
    );
}
