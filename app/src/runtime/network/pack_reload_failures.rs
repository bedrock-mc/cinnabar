use super::*;

#[test]
fn stale_worker_failure_does_not_consume_a_newer_request() {
    let mut app = super::super::pack_reload_tests::app_with_assets(Arc::new(
        assets::RuntimeAssets::diagnostic(),
    ));
    let (tx, rx) = mpsc::sync_channel(1);
    {
        let mut reload = app.world_mut().resource_mut::<PackReload>();
        reload.request_globals(resource_pack::validate_handoff(
            protocol::ResourcePackHandoff::default(),
        ));
        reload.pending = Some(Pending {
            revision: 0,
            generation: 0,
            result: Mutex::new(rx),
        });
    }
    tx.send(Err("stale failure".into())).unwrap();
    app.update();
    let reload = app.world().resource::<PackReload>();
    assert!(reload.error().is_none());
    assert!(reload.progress().is_some());
    assert!(reload.pending.is_some());
}

#[test]
fn stale_session_worker_disconnect_does_not_fail_the_current_session() {
    let mut app = super::super::pack_reload_tests::app_with_assets(Arc::new(
        assets::RuntimeAssets::diagnostic(),
    ));
    let (tx, rx) = mpsc::sync_channel(1);
    {
        let mut reload = app.world_mut().resource_mut::<PackReload>();
        reload.generation = 2;
        reload.revision = 1;
        reload.pending = Some(Pending {
            revision: 1,
            generation: 1,
            result: Mutex::new(rx),
        });
    }
    drop(tx);
    app.update();
    let reload = app.world().resource::<PackReload>();
    assert!(reload.error().is_none());
    assert!(reload.progress().is_some());
    assert!(reload.pending.is_some());
}
