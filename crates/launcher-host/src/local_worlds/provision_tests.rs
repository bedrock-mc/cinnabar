use std::sync::atomic::Ordering;

use super::*;
use crate::local_worlds::test_support::{FakeMojang, build_zip, scratch};

const MAC: Platform = Platform {
    os: "darwin",
    arch: "arm64",
};

#[test]
fn only_windows_and_linux_x86_64_run_bds() {
    for (os, arch, runs) in [
        ("windows", "amd64", true),
        ("linux", "amd64", true),
        ("darwin", "amd64", false),
        ("freebsd", "amd64", false),
        ("linux", "arm64", false),
    ] {
        assert_eq!(Platform { os, arch }.runs_bds(), runs, "{os}/{arch}");
    }
}

#[test]
fn ensure_requires_the_eula_then_downloads_and_records_provenance() {
    let dir = scratch();
    let mojang = FakeMojang::start(
        "1.26.52.3",
        build_zip(&[("bedrock_server", b"bin"), ("server.properties", b"x")]),
    );
    let provisioner = mojang.provisioner(dir.path());
    let (setup, _) = provisioner.status();
    assert_eq!(setup.state, SetupState::EulaRequired);
    assert!(!setup.eula_accepted);
    assert!(matches!(
        provisioner.ensure(&Cancel::never()),
        Err(WorldError::EulaRequired)
    ));
    assert_eq!(
        mojang.hits.load(Ordering::SeqCst),
        0,
        "downloaded before the EULA"
    );
    provisioner.accept_eula().expect("accept");
    assert_eq!(provisioner.status().0.state, SetupState::NotInstalled);
    let binary = provisioner.ensure(&Cancel::never()).expect("ensure");
    let install = binary.parent().expect("install folder");
    assert_eq!(
        install.file_name().and_then(|name| name.to_str()),
        Some("1.26.52.3")
    );
    let manifest = fs::read_to_string(install.join("manifest.json")).expect("manifest");
    assert!(manifest.contains("\"zip_sha256\": \""), "{manifest}");
    assert!(
        manifest.contains("/bin-linux/bedrock-server-1.26.52.3.zip"),
        "{manifest}"
    );
    let (setup, _) = provisioner.status();
    assert_eq!(
        (setup.state, setup.version.as_deref()),
        (SetupState::Ready, Some("1.26.52.3"))
    );
    let hits = mojang.hits.load(Ordering::SeqCst);
    provisioner.ensure(&Cancel::never()).expect("reuse");
    assert_eq!(
        mojang.hits.load(Ordering::SeqCst),
        hits,
        "a second ensure must reuse the install"
    );
    let left = fs::read_dir(provisioner.root.join("downloads")).map_or(0, Iterator::count);
    assert_eq!(left, 0, "download left behind");
}

#[test]
fn ensure_refuses_a_build_of_another_release_line() {
    let dir = scratch();
    let mojang = FakeMojang::start("1.27.0.2", build_zip(&[("bedrock_server", b"bin")]));
    let provisioner = mojang.provisioner(dir.path());
    provisioner.accept_eula().expect("accept");
    let error = provisioner
        .ensure(&Cancel::never())
        .expect_err("version mismatch");
    assert!(error.to_string().contains("1.26.52"), "{error}");
    let (setup, _) = provisioner.status();
    assert_eq!(setup.state, SetupState::Failed);
    assert!(!setup.error.unwrap_or_default().contains("http"));
}

#[test]
fn ensure_rejects_escaping_entries_and_archives_without_a_server() {
    let cases: [&[(&str, &[u8])]; 3] = [
        &[("../evil", b"x"), ("bedrock_server", b"bin")],
        &[("readme.txt", b"x")],
        &[],
    ];
    for files in cases {
        let dir = scratch();
        let mojang = FakeMojang::start("1.26.52.3", build_zip(files));
        let provisioner = mojang.provisioner(dir.path());
        provisioner.accept_eula().expect("accept");
        assert!(provisioner.ensure(&Cancel::never()).is_err(), "{files:?}");
        assert!(
            provisioner.installed().is_none(),
            "a partial install counted"
        );
        assert!(
            fs::metadata(dir.path().join("evil")).is_err(),
            "the archive escaped the install folder"
        );
    }
}

#[test]
fn ensure_refuses_unofficial_hosts_and_unsupported_platforms() {
    let dir = scratch();
    let mut provisioner = Provisioner::new(dir.path().join("linux"), String::new());
    provisioner.platform = Platform {
        os: "linux",
        arch: "amd64",
    };
    provisioner.version_prefix = "1.26.52".into();
    provisioner.links_url = "https://evil.example/links".into();
    provisioner.accept_eula().expect("accept");
    let error = provisioner
        .ensure(&Cancel::never())
        .expect_err("unofficial host");
    assert!(error.to_string().contains("non-official"), "{error}");
    let mut mac = Provisioner::new(dir.path().join("mac"), String::new());
    mac.platform = MAC;
    assert!(matches!(
        mac.ensure(&Cancel::never()),
        Err(WorldError::BackendUnavailable)
    ));
    assert_eq!(mac.status().0.state, SetupState::Unsupported);
}

#[test]
fn an_exact_version_must_match_the_client_line() {
    let dir = scratch();
    let mut provisioner = Provisioner::new(dir.path().join("bds"), "1.27.1.0".into());
    provisioner.platform = Platform {
        os: "linux",
        arch: "amd64",
    };
    provisioner.version_prefix = "1.26.52".into();
    provisioner.accept_eula().expect("accept");
    let error = provisioner.ensure(&Cancel::never()).expect_err("mismatch");
    assert!(error.to_string().contains("does not match"), "{error}");
}

/// Without a pinned version nothing is fetched: the current download may speak another protocol.
#[test]
fn nothing_is_fetched_without_a_pinned_version() {
    let dir = scratch();
    let mojang = FakeMojang::start("1.26.52.3", build_zip(&[("bedrock_server", b"bin")]));
    let mut provisioner = mojang.provisioner(dir.path());
    provisioner.version_prefix = String::new();
    provisioner.accept_eula().expect("accept");
    let error = provisioner.ensure(&Cancel::never()).expect_err("unpinned");
    assert!(error.to_string().contains("pinned"), "{error}");
    assert_eq!(mojang.hits.load(Ordering::SeqCst), 0);
    let pinned = Provisioner::new(dir.path().join("pinned"), "1.26.52.3".into());
    assert_eq!(pinned.prefix(), "1.26.52");
}

/// Startup must not wait on `docker info`: status reports `checking_runtime` until the probe lands.
#[test]
fn background_detection_reports_checking_then_its_result() {
    let dir = scratch();
    let mut provisioner = Provisioner::new(dir.path().join("bds"), String::new());
    provisioner.platform = MAC;
    let provisioner = Arc::new(provisioner);
    let (answer, probe) = crossbeam_channel::bounded::<RuntimeInfo>(0);
    provisioner.set_detector(Arc::new(move || probe.recv().expect("probe answer")));
    provisioner.detect_in_background(RuntimeInfo::checking());
    let (setup, unavailable) = provisioner.status();
    assert_eq!(
        (setup.state, setup.runtime.as_str(), unavailable),
        (SetupState::CheckingRuntime, "container", None)
    );
    answer
        .send(RuntimeInfo::none(
            "down",
            Some(UnavailableReason::DockerNotRunning),
        ))
        .expect("answer");
    assert!(provisioner.await_runtime(test_time::DEFAULT_TIMEOUT));
    let (setup, unavailable) = provisioner.status();
    assert_eq!(
        (setup.state, unavailable),
        (
            SetupState::Unsupported,
            Some(UnavailableReason::DockerNotRunning)
        )
    );
}
