use super::*;
use crate::local_worlds::test_support::{NativeHostFixture, paired};
use protocol::{ClientRandomId, LoginSequence, LoginSettings, SessionLogin, WorldEvent};

#[test]
fn local_selection_uses_the_actual_endpoint_and_cannot_close_a_replacement_opening() {
    let world = NativeHostFixture::new();
    let selection = world.manager.native_local_world(&world.world_id).unwrap();
    let target = selection.connection_target().unwrap();
    assert_eq!(
        target.address,
        world.manager.core_report().target.unwrap().address
    );
    assert_eq!(target.transport, protocol::NativeTransport::RakNet);
    world.manager.close();
    test_time::eventually("original world stopped", || world.runner.last().stopped());
    world.manager.open(&world.world_id, None).unwrap();
    test_time::eventually("replacement world running", || {
        world.manager.status().state == WorldState::Running
    });
    let replacement = world.runner.last();
    assert_eq!(
        selection.validate().unwrap_err().kind(),
        io::ErrorKind::NotConnected
    );
    selection.request_close();
    assert!(
        !replacement.stopped(),
        "old session stopped a replacement process"
    );
    assert_eq!(world.manager.status().state, WorldState::Running);
    let current = world.manager.native_local_world(&world.world_id).unwrap();
    current.request_close();
    test_time::eventually("selected replacement saved and stopped", || {
        replacement.stopped()
    });
}

#[tokio::test]
async fn local_selection_observes_original_process_retirement_without_a_polling_task() {
    use std::{
        future::{Future, poll_fn},
        task::Poll,
    };
    let world = NativeHostFixture::new();
    let mut selection = world.manager.native_local_world(&world.world_id).unwrap();
    let mut changed = Box::pin(selection.changed());
    assert!(
        poll_fn(|cx| Poll::Ready(changed.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    world.runner.last().exit();
    let error = tokio::time::timeout(protocol::NATIVE_JOIN_TIMEOUT, changed)
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::NotConnected);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn local_player_native_session_reaches_the_actual_dragonfly_world_without_a_core_route() {
    let Some(world) = paired::World::open() else {
        return;
    };
    tokio::time::timeout(protocol::NATIVE_JOIN_TIMEOUT, async {
        let id = world.manager.status().world_id.unwrap();
        let selection = world.manager.native_local_world(&id).unwrap();
        let target = selection.connection_target().unwrap();
        let settings = LoginSettings {
            view_distance_chunks: 2,
            ..Default::default()
        };
        let (mut session, (archives, game)) = LoginSequence::connect_native(
            &target,
            SessionLogin {
                display_name: "LocalFixture",
                client_random_id: ClientRandomId::new(1).unwrap(),
                cache: None,
                skin: None,
                settings: &settings,
                tap: None,
                relay: None,
                target: None,
            },
            |packs, game| (packs.into_archives(), game),
        )
        .await
        .unwrap();
        session.finish_loading().await.unwrap();
        session
            .send(protocol::request_chunk_radius_packet(3, 3))
            .await
            .unwrap();
        let mut terrain = session.terrain_before_spawn();
        let mut response = false;
        while !(terrain && response) {
            match session.recv_world_event(0).await.unwrap() {
                WorldEvent::ChunkRadiusUpdated(3) => response = true,
                WorldEvent::LevelChunk(_) | WorldEvent::SubChunks(_) => terrain = true,
                _ => {}
            }
        }
        session.close().await.unwrap();
        assert!(
            archives
                .iter()
                .any(|pack| pack.pack_id == world.pack && pack.archive == world.archive),
            "original final-resource archive was not delivered unchanged"
        );
        assert!(
            game.world.local_player_runtime_id != 0 && terrain && response,
            "actual world initialization/terrain/live response was missing"
        );
        selection.validate().unwrap();
        selection.request_close();
        test_time::eventually("actual selected process stopped", || {
            world.manager.status().state == WorldState::Idle
        });
    })
    .await
    .expect("finite local-player native fixture stalled");
}
