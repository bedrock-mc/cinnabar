//! Host lookup and operator reconciliation while a server starts.

use super::*;
use crate::local_worlds::process::HostGrant;

/// A manager whose EULA is accepted, with one BDS world served by `runner`.
fn bds_world(runner: &Arc<FakeRunner>) -> (Fixture, World) {
    let runner: Arc<dyn Runner> = runner.clone();
    let fixture = fixture(|parts, dir| {
        parts.runners.bds = Some(runner);
        parts.setup = Some(provisioner(dir, LINUX));
    });
    fixture.manager.accept_eula().expect("accept");
    let world = fixture
        .manager
        .create(&NewWorld {
            backend: Some(Backend::Bds),
            ..flat("w")
        })
        .expect("create");
    (fixture, world)
}

/// Each start hosts the account signed in at that moment, and a malformed XUID grants nobody. A
/// host known at launch needs no reload once the server is ready.
#[test]
fn starts_carry_the_signed_in_host() {
    let runner = Arc::new(FakeRunner::default());
    let (fixture, world) = bds_world(&runner);
    let manager = &fixture.manager;
    for host in ["2535400000000001", "2535 OR 1", ""] {
        manager.set_host(if host.is_empty() {
            Host::SignedOut
        } else {
            Host::Xuid(host.to_owned())
        });
        manager.open(&world.id, None).expect("open");
        wait_for(manager, WorldState::Running);
        manager.close();
        wait_for(manager, WorldState::Idle);
    }
    assert_eq!(
        *lock(&runner.hosts),
        [Some("2535400000000001".to_owned()), None, None]
    );
    for instance in lock(&runner.made).iter() {
        assert!(
            lock(&instance.grants).is_empty(),
            "an unchanged host was regranted"
        );
    }
}

/// An open made while a new sign-in's XUID loads launches BDS at once, then grants the host
/// exactly once before the world is reported running, so the host still joins as operator.
#[test]
fn a_bds_server_starts_while_the_host_loads() {
    let (runner, release_grant) = FakeRunner::granting();
    let runner = Arc::new(runner);
    let (fixture, world) = bds_world(&runner);
    let manager = &fixture.manager;
    manager.set_host(Host::Loading);
    manager.open(&world.id, None).expect("open");
    test_time::eventually("the server starts while the host loads", || {
        !lock(&runner.made).is_empty()
    });
    assert_eq!(
        *lock(&runner.hosts),
        [None],
        "launched before the host was known"
    );
    waiting_on_host(manager);
    assert_eq!(manager.status().state, WorldState::Starting);
    let instance = runner.last();
    assert!(lock(&instance.grants).is_empty());
    manager.set_host(Host::Xuid("2535400000000001".to_owned()));
    test_time::eventually("the host is granted", || !lock(&instance.grants).is_empty());
    assert_eq!(
        manager.status().state,
        WorldState::Starting,
        "running was reported before the grant finished"
    );
    drop(release_grant);
    wait_for(manager, WorldState::Running);
    assert_eq!(
        *lock(&instance.grants),
        [Some("2535400000000001".to_owned())]
    );
}

/// A container's operator list is fixed at launch, so its start still waits for a loading host.
#[test]
fn a_container_start_waits_for_a_loading_host_before_launching() {
    let runner = Arc::new(FakeRunner {
        host_grant: Some(HostGrant::AtLaunch),
        ..FakeRunner::default()
    });
    let (fixture, world) = bds_world(&runner);
    let manager = &fixture.manager;
    manager.set_host(Host::Loading);
    manager.open(&world.id, None).expect("open");
    waiting_on_host(manager);
    assert_eq!(runner.entered.load(std::sync::atomic::Ordering::SeqCst), 0);
    manager.set_host(Host::Xuid("2535400000000001".to_owned()));
    wait_for(manager, WorldState::Running);
    assert_eq!(*lock(&runner.hosts), [Some("2535400000000001".to_owned())]);
    assert!(lock(&runner.last().grants).is_empty());
}

/// Closing a world whose launched server waits on the host stops that server before idling.
#[test]
fn closing_during_the_host_wait_stops_the_started_server() {
    let runner = Arc::new(FakeRunner::default());
    let (fixture, world) = bds_world(&runner);
    let manager = &fixture.manager;
    manager.set_host(Host::Loading);
    manager.open(&world.id, None).expect("open");
    waiting_on_host(manager);
    let instance = runner.last();
    manager.close();
    wait_for(manager, WorldState::Idle);
    assert!(instance.stopped(), "the started server was left running");
    assert!(lock(&instance.grants).is_empty());
}

/// A server crash during profile loading fails the world without waiting for the profile.
#[test]
fn a_server_exit_ends_the_host_wait() {
    let runner = Arc::new(FakeRunner::default());
    let (fixture, world) = bds_world(&runner);
    let manager = &fixture.manager;
    manager.set_host(Host::Loading);
    manager.open(&world.id, None).expect("open");
    waiting_on_host(manager);
    runner.last().exit();
    wait_for(manager, WorldState::Failed);
    assert!(manager.core_report().target.is_none());
}

/// Cancellation also ends a permission reload already in flight and saves the started server.
#[test]
fn closing_while_the_host_grant_is_pending_stops_the_server() {
    let (runner, _release) = FakeRunner::granting();
    let runner = Arc::new(runner);
    let (fixture, world) = bds_world(&runner);
    let manager = &fixture.manager;
    manager.set_host(Host::Loading);
    manager.open(&world.id, None).expect("open");
    waiting_on_host(manager);
    let instance = runner.last();
    manager.set_host(Host::Xuid("2535400000000001".to_owned()));
    test_time::eventually("the grant starts", || !lock(&instance.grants).is_empty());
    manager.close();
    wait_for(manager, WorldState::Idle);
    assert!(instance.stopped());
}

/// Closing a container start that waits on the host cancels it before any server starts.
#[test]
fn closing_ends_a_container_wait_for_the_host_before_launch() {
    let runner = Arc::new(FakeRunner {
        host_grant: Some(HostGrant::AtLaunch),
        ..FakeRunner::default()
    });
    let (fixture, world) = bds_world(&runner);
    let manager = &fixture.manager;
    manager.set_host(Host::Loading);
    manager.open(&world.id, None).expect("open");
    waiting_on_host(manager);
    manager.close();
    wait_for(manager, WorldState::Idle);
    assert_eq!(runner.entered.load(std::sync::atomic::Ordering::SeqCst), 0);
}

/// A host that signs out while loading leaves the server running without an operator or reload.
#[test]
fn a_sign_out_during_the_host_wait_runs_without_a_grant() {
    let runner = Arc::new(FakeRunner::default());
    let (fixture, world) = bds_world(&runner);
    let manager = &fixture.manager;
    manager.set_host(Host::Loading);
    manager.open(&world.id, None).expect("open");
    waiting_on_host(manager);
    manager.set_host(Host::SignedOut);
    wait_for(manager, WorldState::Running);
    assert_eq!(*lock(&runner.hosts), [None]);
    assert!(lock(&runner.last().grants).is_empty());
}

/// A host that changes while the server starts is granted as it is when the server is ready, so
/// the account that launched it keeps no operator.
#[test]
fn a_host_change_during_startup_grants_the_current_account() {
    for (later, granted) in [
        (
            Host::Xuid("2535400000000002".to_owned()),
            Some("2535400000000002"),
        ),
        (Host::SignedOut, None),
    ] {
        let (runner, release) = FakeRunner::gated();
        let runner = Arc::new(runner);
        let (fixture, world) = bds_world(&runner);
        let manager = &fixture.manager;
        manager.set_host(Host::Xuid("2535400000000001".to_owned()));
        manager.open(&world.id, None).expect("open");
        test_time::eventually("the server launches", || {
            runner.entered.load(std::sync::atomic::Ordering::SeqCst) == 1
        });
        manager.set_host(later);
        drop(release);
        wait_for(manager, WorldState::Running);
        assert_eq!(*lock(&runner.hosts), [Some("2535400000000001".to_owned())]);
        assert_eq!(*lock(&runner.last().grants), [granted.map(str::to_owned)]);
    }
}

/// Account changes during the console acknowledgement replace the pending grant before Running.
#[test]
fn a_host_change_during_a_pending_grant_is_reconciled() {
    for (later, expected) in [
        (Host::SignedOut, None),
        (
            Host::Xuid("2535400000000002".to_owned()),
            Some("2535400000000002"),
        ),
    ] {
        let (runner, release) = FakeRunner::granting();
        let runner = Arc::new(runner);
        let (fixture, world) = bds_world(&runner);
        let manager = &fixture.manager;
        manager.set_host(Host::Loading);
        manager.open(&world.id, None).expect("open");
        waiting_on_host(manager);
        let instance = runner.last();
        manager.set_host(Host::Xuid("2535400000000001".to_owned()));
        test_time::eventually("the first grant is pending", || {
            !lock(&instance.grants).is_empty()
        });
        manager.set_host(later);
        drop(release);
        wait_for(manager, WorldState::Running);
        assert_eq!(
            *lock(&instance.grants),
            [
                Some("2535400000000001".to_owned()),
                expected.map(str::to_owned)
            ],
            "the obsolete grant must be replaced before Running"
        );
    }
}

/// A grant the server cannot apply stops it gracefully and fails the world.
#[test]
fn a_failed_grant_stops_the_server_and_fails_the_world() {
    let runner = Arc::new(FakeRunner {
        grant_error: Some(|| WorldError::Failed("reload /secret/path: closed".into())),
        ..FakeRunner::default()
    });
    let (fixture, world) = bds_world(&runner);
    let manager = &fixture.manager;
    manager.set_host(Host::Loading);
    manager.open(&world.id, None).expect("open");
    waiting_on_host(manager);
    manager.set_host(Host::Xuid("2535400000000001".to_owned()));
    let status = wait_for(manager, WorldState::Failed);
    let instance = runner.last();
    assert!(instance.stopped(), "the ungranted server was left running");
    assert_eq!(
        *lock(&instance.grants),
        [Some("2535400000000001".to_owned())]
    );
    let error = status.error.expect("failure text");
    assert!(!error.contains("secret"), "{error}");
    assert!(manager.core_report().target.is_none());
}

/// Dragonfly grants no operator, so its starts never wait on a loading host.
#[test]
fn dragonfly_starts_do_not_wait_for_the_host() {
    let runner = Arc::new(FakeRunner::default());
    let runner_dyn: Arc<dyn Runner> = runner.clone();
    let fixture = fixture(|parts, _| {
        parts.runners.dragonfly = Some(runner_dyn);
        parts.host_wait = Duration::from_secs(3600);
    });
    let manager = &fixture.manager;
    let world = manager
        .create(&NewWorld {
            backend: Some(Backend::Dragonfly),
            ..flat("w")
        })
        .expect("create");
    manager.set_host(Host::Loading);
    manager.open(&world.id, None).expect("open");
    wait_for(manager, WorldState::Running);
    assert_eq!(*lock(&runner.hosts), [None]);
    assert!(lock(&runner.last().grants).is_empty());
}

/// Waits until a start is blocked on a [`Host::Loading`] host.
#[track_caller]
fn waiting_on_host(manager: &Manager) {
    test_time::eventually("the start waits on the host", || {
        manager
            .inner
            .host_waits
            .load(std::sync::atomic::Ordering::SeqCst)
            == 1
    });
}

/// An XUID that never arrives delays the start only by the host wait, then grants nobody.
#[test]
fn a_host_that_never_loads_starts_without_an_operator() {
    let runner = Arc::new(FakeRunner::default());
    let runner_dyn: Arc<dyn Runner> = runner.clone();
    let fixture = fixture(|parts, dir| {
        parts.runners.bds = Some(runner_dyn);
        parts.setup = Some(provisioner(dir, LINUX));
        parts.host_wait = Duration::ZERO;
    });
    let manager = &fixture.manager;
    manager.accept_eula().expect("accept");
    let world = manager
        .create(&NewWorld {
            backend: Some(Backend::Bds),
            ..flat("w")
        })
        .expect("create");
    manager.set_host(Host::Loading);
    manager.open(&world.id, None).expect("open");
    wait_for(manager, WorldState::Running);
    assert_eq!(*lock(&runner.hosts), [None]);
    assert!(lock(&runner.last().grants).is_empty());
}
