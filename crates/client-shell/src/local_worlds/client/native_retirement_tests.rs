use super::*;
use launcher::local_worlds::world::NewWorld;

/// Losing a UI reply must not destroy the same request receiver while an original sender remains.
#[test]
fn native_world_worker_retains_original_request_receiver_after_ui_reply_closes() {
    let directory = tempfile::tempdir().unwrap();
    let manager = launcher_host::local_worlds::open_store(directory.path().join("worlds")).unwrap();
    let world = manager
        .create(&NewWorld {
            name: "Original cleanup".into(),
            ..Default::default()
        })
        .unwrap();
    manager.save_thumbnail(&world.id, b"original view").unwrap();
    let WorldsClient {
        effects,
        events,
        revoked,
        _core,
        worker,
    } = WorldsClient::spawn_native(manager.clone()).unwrap();
    // This is the actual sender a retirement capability retains, not a replacement channel/worker.
    let retained = effects.clone();
    drop(effects);
    drop(events);
    retained.send(Request::Effect(Effect::List)).unwrap();
    // FIFO forces the original failed reply before this existing worker-owned persisted effect.
    let _ = retained.send(Request::SaveThumbnail {
        id: world.id.clone(),
        image: Arc::from(&b"retired view"[..]),
    });
    drop(retained);
    worker.unwrap().join().unwrap();
    drop((_core, revoked));
    let observed = manager.thumbnail(&world.id).unwrap();
    manager.close();
    drop(manager);
    assert_eq!(
        observed, b"retired view",
        "lost UI reply retired the original cleanup request receiver"
    );
}
