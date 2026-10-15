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
    let (sent, received) = bounded(1);
    sent.send(ServiceResult::Done(ServicedStream {
        stream: stream(),
        report,
        busy: Duration::ZERO,
        held: Duration::ZERO,
    }))
    .unwrap();
    let mut service = WorldStreamService::spawn().unwrap();
    service.results = received;
    service.servicing = true;
    let capture = Capture::default();
    let serviced = capture.with(|| service.reclaim()).unwrap();
    assert_eq!(serviced.report, report);
    let observations = capture.observations();
    let summary = observations
        .iter()
        .find(|span| span.entered && span.name == "stream.service_received")
        .or_else(|| {
            observations
                .iter()
                .find(|span| span.entered && span.name == "stream.service_reclaim")
        })
        .expect("a receive or reclaim span");
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

/// A queued reclaim wake is observed without consuming any worker result or sleeping.
#[test]
fn idle_wait_identifies_the_reclaim_wake_on_entry() {
    let stream = stream();
    let (sent, wake) = bounded(1);
    sent.send(()).unwrap();
    let capture = Capture::default();
    capture.with(|| stream.wait_for_worker_result(&wake, true, Duration::ZERO));
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
        wake.try_recv().is_ok(),
        "readiness observation does not consume the wake"
    );
    assert!(stream.decode_rx.is_empty());
    assert!(stream.lighting.rx.is_empty());
    assert!(stream.mesh_rx.is_empty());
}

/// A yielded run exports the launch identity and completed report before publishing it.
#[test]
fn service_return_exports_its_handoff_identity_on_entry() {
    let (sent, launches) = bounded(1);
    sent.send(ServiceLaunch {
        handoff_id: 42,
        stream: stream(),
        camera_position: [0.0; 3],
        max_mesh_jobs: 0,
    })
    .unwrap_or_else(|_| panic!("the launch receiver is live"));
    drop(sent);
    let (results, received) = bounded(1);
    let (_wake, wake) = bounded(1);
    let requested = Arc::new(AtomicBool::new(true));
    let capture = Capture::default();
    capture.with(|| serve(&launches, &results, &wake, &requested));
    let ServiceResult::Done(serviced) = received.try_recv().unwrap() else {
        panic!("the yielded stream must be returned");
    };
    assert_eq!(serviced.report, WorldStreamPoll::default());
    for name in ["stream.service_run", "stream.service_return"] {
        let observations = capture.observations();
        let span = observations
            .iter()
            .find(|span| span.entered && span.name == name)
            .unwrap();
        assert_eq!(
            span.fields.get("handoff_id").map(String::as_str),
            Some("42")
        );
        if name == "stream.service_return" {
            assert_eq!(
                span.fields.get("commit_steps").map(String::as_str),
                Some("0")
            );
            assert_eq!(
                span.fields.get("maintenance_items").map(String::as_str),
                Some("0")
            );
            assert_eq!(
                span.fields.get("service_item_max_ms").map(String::as_str),
                Some("0.0")
            );
        }
    }
}
