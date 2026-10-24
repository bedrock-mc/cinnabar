use std::{fs, sync::atomic::Ordering};

use super::*;
use crate::local_worlds::{
    provision::RuntimeInfo,
    test_support::{FakeMojang, VERSION, build_zip, scratch},
};

#[test]
fn download_and_preparation_finish_before_ready_and_reuse_the_install() {
    let directory = scratch();
    let mojang = FakeMojang::start(VERSION, build_zip(&[("bedrock_server", b"fixture")]));
    let provisioner = mojang.provisioner(directory.path());
    provisioner.set_runtime(RuntimeInfo::translated());
    let (prepared, awaited) = crossbeam_channel::bounded(0);
    let (release, proceed) = crossbeam_channel::bounded(0);
    provisioner.set_preparer(
        Arc::new(move |binary, _, _| {
            let marker = binary.with_extension("prepared");
            if !marker.exists() {
                prepared.send(()).unwrap();
                proceed.recv().unwrap();
                fs::write(marker, b"prepared").unwrap();
            }
            Ok(())
        }),
        Arc::new(|binary| binary.with_extension("prepared").is_file()),
    );
    provisioner.accept_eula().unwrap();
    let cancelled = AtomicBool::new(false);
    thread::scope(|scope| {
        let work = scope.spawn(|| run_setup(&provisioner, &cancelled, &mut |_| {}));
        awaited.recv_timeout(test_time::DEFAULT_TIMEOUT).unwrap();
        assert_ne!(provisioner.status().0.state, SetupState::Ready);
        release.send(()).unwrap();
        work.join().unwrap().unwrap();
    });
    assert_eq!(provisioner.status().0.state, SetupState::Ready);
    let hits = mojang.hits.load(Ordering::SeqCst);
    run_setup(&provisioner, &cancelled, &mut |_| {}).unwrap();
    assert_eq!(mojang.hits.load(Ordering::SeqCst), hits);
    assert_eq!(provisioner.status().0.state, SetupState::Ready);
}

#[test]
fn preparation_failure_does_not_report_an_installed_binary_ready() {
    let directory = scratch();
    let mojang = FakeMojang::start(VERSION, build_zip(&[("bedrock_server", b"fixture")]));
    let provisioner = mojang.provisioner(directory.path());
    provisioner.set_runtime(RuntimeInfo::translated());
    provisioner.accept_eula().unwrap();
    provisioner.set_preparer(
        Arc::new(|_, _, _| Err(failed("fixture preparation failed"))),
        Arc::new(|_| false),
    );
    assert!(run_setup(&provisioner, &AtomicBool::new(false), &mut |_| {}).is_err());
    assert!(provisioner.installed().is_some());
    assert_eq!(provisioner.status().0.state, SetupState::Failed);
    assert_eq!(
        provisioner.status().0.error.as_deref(),
        Some("local world preparation failed")
    );
}

#[test]
fn cancelled_setup_does_not_download_and_cancel_during_preparation_stops_it() {
    let directory = scratch();
    let mojang = FakeMojang::start(VERSION, build_zip(&[("bedrock_server", b"fixture")]));
    let provisioner = mojang.provisioner(directory.path());
    provisioner.set_runtime(RuntimeInfo::translated());
    provisioner.accept_eula().unwrap();
    assert!(matches!(
        run_setup(&provisioner, &AtomicBool::new(true), &mut |_| {}),
        Err(WorldError::Cancelled)
    ));
    assert_eq!(mojang.hits.load(Ordering::SeqCst), 0);
    let cancelled = AtomicBool::new(false);
    let started = Arc::new(AtomicBool::new(false));
    let preparing = Arc::clone(&started);
    provisioner.set_preparer(
        Arc::new(move |_, _, cancel| {
            preparing.store(true, Ordering::Release);
            cancel.receiver().recv().unwrap_err();
            Err(WorldError::Cancelled)
        }),
        Arc::new(|_| false),
    );
    let result = run_setup(&provisioner, &cancelled, &mut |line| {
        if line == "Preparing local worlds" && started.load(Ordering::Acquire) {
            cancelled.store(true, Ordering::Release);
        }
    });
    assert!(matches!(result, Err(WorldError::Cancelled)));
    assert_ne!(provisioner.status().0.state, SetupState::Ready);
}
