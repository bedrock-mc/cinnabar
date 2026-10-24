use super::*;
use crate::stream::trace_test_support::Capture;

/// Creates a stream without external fixtures or submitted worker jobs.
fn stream() -> WorldStream {
    WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    })
}

/// Report values must already exist when the receive summary enters its trace zone.
#[test]
fn reclaim_summary_exports_completed_values_on_entry() {
    let report = WorldStreamPoll {
        service_item_max: Duration::from_micros(2500),
        maintenance_items: 7,
        commit_steps: 3,
        ..Default::default()
    };
    let mut service = WorldStreamService::spawn().unwrap();
    let shared = Arc::clone(&service.shared);
    let turns = shared.turns.lock().unwrap();
    service.launch(stream(), [0.0; 3], 0);
    shared.slot().lent.as_mut().unwrap().report = report;
    let capture = Capture::default();
    let serviced = capture.with(|| service.reclaim()).unwrap();
    drop(turns);
    assert_eq!(serviced.report, report);
    let observations = capture.observations();
    let summary = observations
        .iter()
        .find(|span| span.entered && span.name == "stream.service_received")
        .expect("a receive span");
    assert_eq!(
        summary
            .fields
            .get("service_item_max_ms")
            .map(String::as_str),
        Some("2.5")
    );
    assert_eq!(
        summary.fields.get("maintenance_items").map(String::as_str),
        Some("7")
    );
    assert_eq!(
        summary.fields.get("commit_steps").map(String::as_str),
        Some("3")
    );
}

/// A queued wake is observed and consumed without consuming any worker result or sleeping.
#[test]
fn idle_wait_identifies_the_reclaim_wake_on_entry() {
    let stream = stream();
    let (sent, wake) = bounded(1);
    sent.send(()).unwrap();
    let capture = Capture::default();
    capture.with(|| stream.idle_wait().wait(&wake, false, Duration::ZERO));
    let observations = capture.observations();
    let woke = observations
        .iter()
        .find(|span| span.entered && span.name == "stream.service_woke")
        .unwrap();
    assert_eq!(
        woke.fields.get("source").map(String::as_str),
        Some("\"reclaim\"")
    );
    assert!(
        wake.try_recv().is_err(),
        "the observed wake is consumed so it cannot cut later waits short"
    );
    assert!(stream.decode_rx.is_empty());
    assert!(stream.lighting.rx.is_empty());
    assert!(stream.mesh_rx.is_empty());
}

/// A service slice exports its loan's handoff identity.
#[test]
fn service_run_exports_its_handoff_identity_on_entry() {
    let shared = Arc::new(Shared {
        slot: Mutex::new(Slot {
            lent: Some(Lent {
                handoff_id: 42,
                stream: stream(),
                camera_position: [0.0; 3],
                max_mesh_jobs: 0,
                report: WorldStreamPoll::default(),
                busy: Duration::ZERO,
                serving_since: None,
            }),
            ..Slot::default()
        }),
        yield_request: Arc::new(AtomicBool::new(false)),
        turns: Mutex::new(()),
    });
    let reclaimer = std::thread::spawn({
        let shared = Arc::clone(&shared);
        move || {
            test_time::eventually("the service polls once", || {
                shared
                    .slot()
                    .lent
                    .as_ref()
                    .is_some_and(|lent| lent.serving_since.is_some())
            });
            shared.yield_request.store(true, Ordering::Release);
        }
    });
    let (_wake, wake) = bounded(1);
    let capture = Capture::default();
    let shutting_down = capture.with(|| service_lent(&shared, &wake));
    reclaimer.join().unwrap();
    assert!(!shutting_down);
    let observations = capture.observations();
    let span = observations
        .iter()
        .find(|span| span.entered && span.name == "stream.service_run")
        .unwrap();
    assert_eq!(
        span.fields.get("handoff_id").map(String::as_str),
        Some("42")
    );
}
