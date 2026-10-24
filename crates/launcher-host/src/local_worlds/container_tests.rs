use super::*;
#[cfg(unix)]
use crate::local_worlds::{
    process::HostGrant,
    provision::Platform,
    test_support::{FAKE_DOCKER, installed_provisioner, write_script},
};
use crate::local_worlds::{
    process::Runner,
    provision::{Provisioner, RuntimeKind},
    test_support::{VERSION, scratch, spec},
};

const IMAGE: &str = "itzg/minecraft-bedrock-server:2026.9.2@sha256:0000000000000000000000000000000000000000000000000000000000000000";

/// A container-runtime runner whose Linux build is already downloaded, driving a fake Docker CLI
/// that logs to `<dir>/docker.log`.
#[cfg(unix)]
fn container_runner(dir: &Path, image: &str, docker_down: bool) -> (BdsRunner, std::path::PathBuf) {
    let provisioner = installed_provisioner(
        dir,
        Platform {
            os: "darwin",
            arch: "arm64",
        },
        b"elf",
    );
    provisioner.set_runtime(RuntimeInfo {
        kind: RuntimeKind::Container,
        reason: "container".into(),
        unavailable: None,
    });
    let docker = dir.join("docker");
    write_script(&docker, FAKE_DOCKER);
    let log = dir.join("docker.log");
    let mut runner = BdsRunner::new(Arc::new(provisioner), image.to_owned());
    runner.docker = docker.to_string_lossy().into_owned();
    runner
        .env
        .push(("FAKE_DOCKER_LOG".into(), log.clone().into()));
    if docker_down {
        runner.env.push(("FAKE_DOCKER_DOWN".into(), "1".into()));
    }
    runner.start_timeout = Some(test_time::DEFAULT_TIMEOUT);
    (runner, log)
}

#[cfg(unix)]
fn docker_log(log: &Path) -> String {
    fs::read_to_string(log).unwrap_or_default()
}

#[cfg(unix)]
#[test]
fn the_container_runner_starts_stops_and_removes_its_container() {
    let dir = scratch();
    let (runner, log) = container_runner(dir.path(), IMAGE, false);
    assert_eq!(runner.provisioner.status().0.state, SetupState::Ready);
    let world_dir = dir.path().join("world");
    let spec = spec(&world_dir);
    let instance = runner.start(&spec, &Cancel::never()).expect("start");
    assert!(!instance.can_pause(), "container BDS cannot pause");
    let level = fs::read(world_dir.join(LEVEL_DIR).join("level.dat")).expect("level data");
    let has = |tag: &[u8]| level.windows(tag.len()).any(|window| window == tag);
    assert!(
        has(b"Created") && has(b"showcoordinates"),
        "BDS creates the world before owned fields are patched"
    );
    instance.stop(test_time::DEFAULT_TIMEOUT).expect("stop");
    let calls = docker_log(&log);
    let name = container_name(&spec.world.id);
    let install = runner.provisioner.root.join(VERSION);
    for expected in [
        "info\n".to_owned(),
        format!("image inspect {IMAGE}"),
        format!("pull --platform linux/amd64 {IMAGE}"),
        format!("rm -f {name}"),
        format!("run --rm --name {name} --platform linux/amd64 -p 127.0.0.1:"),
        format!(":{BDS_CONTAINER_HTTP_PORT}/tcp"),
        format!("-v {}:/data ", install.display()),
        format!(
            "-v {}:/data/worlds/{}",
            world_dir.join(LEVEL_DIR).display(),
            spec.world.id
        ),
        "-e EULA=TRUE".to_owned(),
        format!("-e VERSION={VERSION}"),
        "-e ONLINE_MODE=false".to_owned(),
        format!("-e FORCE_GAMEMODE={}", super::super::bds::FORCE_GAME_MODE),
        "-e LEVEL_TYPE=FLAT".to_owned(),
        "-e LEVEL_SEED=-7".to_owned(),
        "-e ENABLE_BDS_V6BIND_FIX=TRUE".to_owned(),
        "-e TRANSPORT=nethernet".to_owned(),
        "-e ENABLE_LAN_VISIBILITY=false".to_owned(),
        format!(
            "-e DIRECT_DOWNLOAD_URL=https://www.minecraft.net/bedrockdedicatedserver/bin-linux/bedrock-server-{VERSION}.zip"
        ),
        format!("stop -t 25 {name}"),
    ] {
        assert!(
            calls.contains(&expected),
            "docker log lacks {expected:?}:\n{calls}"
        );
    }
    assert!(
        install.join(format!("bedrock_server-{VERSION}")).is_file(),
        "without the versioned binary the image downloads its own"
    );
    assert!(
        calls.matches(&format!("rm -f {name}")).count() >= 2,
        "{calls}"
    );
    assert_eq!(
        calls.trim_end().lines().last(),
        Some(format!("rm -f {name}").as_str()),
        "cleanup is the last Docker call"
    );
}

/// Docker's run status distinguishes a saved shutdown from a crash or its stop-timeout kill.
#[cfg(unix)]
#[test]
fn only_a_successful_container_exit_is_clean() {
    for status in [0, 3, 137] {
        let dir = scratch();
        let (mut runner, _) = container_runner(dir.path(), IMAGE, false);
        runner
            .env
            .push(("FAKE_DOCKER_EXIT".into(), status.to_string().into()));
        let instance = runner
            .start(&spec(&dir.path().join("world")), &Cancel::never())
            .expect("start");
        assert!(!instance.exited_cleanly(), "still running");
        instance.stop(test_time::DEFAULT_TIMEOUT).expect("stop");
        assert_eq!(instance.exited_cleanly(), status == 0, "exit {status}");
    }
}

/// The image serves the mounted install's operator list, which names only the host. Its console
/// is not attached, so the host is resolved before launch and cannot be granted later.
#[cfg(unix)]
#[test]
fn the_container_runner_makes_only_the_host_an_operator() {
    let dir = scratch();
    let (runner, log) = container_runner(dir.path(), IMAGE, false);
    assert_eq!(runner.host_grant(), HostGrant::AtLaunch);
    let mut spec = spec(&dir.path().join("world"));
    spec.host_xuid = Some("2535400000000001".to_owned());
    let instance = runner.start(&spec, &Cancel::never()).expect("start");
    assert!(
        instance
            .grant_host(Some("2535400000000002"), &Cancel::never())
            .is_err(),
        "a container cannot reload its operators"
    );
    instance.stop(test_time::DEFAULT_TIMEOUT).expect("stop");
    let install = runner.provisioner.root.join(VERSION);
    let file = fs::read(install.join(super::super::bds::PERMISSIONS_FILE)).expect("permissions");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&file).expect("permissions JSON"),
        serde_json::json!([{ "permission": "operator", "xuid": "2535400000000001" }])
    );
    let calls = docker_log(&log);
    assert!(
        calls.contains(&format!("-v {}:/data ", install.display())),
        "{calls}"
    );
    assert!(
        !calls.contains("-e OPS="),
        "the image would replace the list: {calls}"
    );
}

/// A crashed run's container still holds its ports, so it goes before ports are reserved.
#[cfg(unix)]
#[test]
fn a_leftover_container_is_removed_before_ports_are_reserved() {
    let dir = scratch();
    let (mut runner, log) = container_runner(dir.path(), IMAGE, false);
    let held = std::net::TcpListener::bind((super::super::ports::LOCAL_SERVER_HOST, 0)).unwrap();
    runner.host_port = held.local_addr().unwrap().port();
    let spec = spec(&dir.path().join("world"));
    assert!(runner.start(&spec, &Cancel::never()).is_err());
    let name = container_name(&spec.world.id);
    assert!(
        docker_log(&log).contains(&format!("rm -f {name}")),
        "{}",
        docker_log(&log)
    );
}

#[cfg(unix)]
#[test]
fn a_present_image_is_not_pulled_again() {
    let dir = scratch();
    let (runner, log) = container_runner(dir.path(), IMAGE, false);
    let mut pulled = log.into_os_string();
    pulled.push(".pulled");
    fs::write(pulled, b"").expect("mark the image present");
    let instance = runner
        .start(&spec(&dir.path().join("world")), &Cancel::never())
        .expect("start");
    instance.stop(test_time::DEFAULT_TIMEOUT).expect("stop");
    assert!(!docker_log(&dir.path().join("docker.log")).contains("pull "));
}

/// An image without a digest drifts with upstream releases, so Docker is never touched for it.
#[cfg(unix)]
#[test]
fn unpinned_images_are_refused_before_docker_runs() {
    let dir = scratch();
    for image in [
        "",
        "itzg/minecraft-bedrock-server:latest",
        "itzg/minecraft-bedrock-server:2026.9.2",
    ] {
        let (runner, log) = container_runner(dir.path(), image, false);
        let started = runner.start(&spec(&dir.path().join("world")), &Cancel::never());
        assert!(
            matches!(started, Err(WorldError::ImageNotPinned)),
            "{image:?}"
        );
        assert!(docker_log(&log).is_empty(), "docker ran for {image:?}");
    }
}

/// A daemon stopped since startup fails the open with the reason the client turns into Retry.
#[cfg(unix)]
#[test]
fn a_stopped_daemon_flips_the_runtime() {
    let dir = scratch();
    let (runner, _) = container_runner(dir.path(), IMAGE, true);
    let started = runner.start(&spec(&dir.path().join("world")), &Cancel::never());
    let error = started.err().expect("Docker is down");
    assert!(matches!(error, WorldError::DockerNotRunning));
    assert_eq!(error.status_text(), "Docker is not running");
    let (setup, unavailable) = runner.provisioner.status();
    assert_eq!(
        (setup.state, unavailable),
        (
            SetupState::Unsupported,
            Some(UnavailableReason::DockerNotRunning)
        )
    );
}

/// Closing a world mid-start ends a hung Docker call instead of waiting out its timeout.
#[cfg(unix)]
#[test]
fn cancelling_a_start_ends_a_hung_docker_call() {
    let dir = scratch();
    let (mut runner, log) = container_runner(dir.path(), IMAGE, false);
    runner.env.push(("FAKE_DOCKER_HANG".into(), "1".into()));
    let (handle, cancel) = crate::local_worlds::cancel_pair();
    let spec = spec(&dir.path().join("world"));
    let (sender, started) = crossbeam_channel::bounded(1);
    thread::spawn(move || {
        let _ = sender.send(runner.start(&spec, &cancel).err());
    });
    test_time::eventually("the hung image check", || {
        docker_log(&log).contains("image inspect")
    });
    drop(handle);
    let error = started
        .recv_timeout(test_time::DEFAULT_TIMEOUT)
        .expect("the start ended")
        .expect("a cancelled start");
    assert!(matches!(error, WorldError::Cancelled), "{error}");
}

#[test]
fn the_container_runner_needs_the_eula() {
    let dir = scratch();
    let provisioner = Provisioner::new(dir.path().join("bds"), VERSION.into());
    provisioner.set_runtime(RuntimeInfo {
        kind: RuntimeKind::Container,
        reason: "container".into(),
        unavailable: None,
    });
    let runner = BdsRunner::new(Arc::new(provisioner), IMAGE.into());
    let started = runner.start(&spec(dir.path()), &Cancel::never());
    assert!(matches!(started, Err(WorldError::EulaRequired)));
}

#[test]
fn pull_progress_counts_layers() {
    let mut progress = PullProgress::default();
    let mut counts = (0, 0);
    for line in [
        "2026.9.2: Pulling from itzg/minecraft-bedrock-server",
        "aaaaaaaaaaaa: Already exists",
        "bbbbbbbbbbbb: Pulling fs layer",
        "cccccccccccc: Pulling fs layer",
        "bbbbbbbbbbbb: Download complete",
        "bbbbbbbbbbbb: Pull complete",
        "bbbbbbbbbbbb: Pull complete",
        "Digest: sha256:0000",
    ] {
        counts = progress.line(line);
    }
    assert_eq!(counts, (2, 3));
}

fn args(spec: &StartSpec, players: u32, lan_visible: bool, lan_host_port: u16) -> Vec<String> {
    container_args(&ContainerArgs {
        spec,
        image: IMAGE,
        version: "fixture-version",
        install: Path::new("/install"),
        host_port: 5000,
        max_players: players,
        lan_visible,
        lan_host_port,
    })
}

/// The published ports match the NetherNet candidates BDS advertises, for every player limit.
#[test]
fn container_ports_match_the_advertised_candidates() {
    let dir = scratch();
    let spec = spec(dir.path());
    for players in [1, 8] {
        let args = args(&spec, players, false, 0);
        for expected in [
            format!("127.0.0.1:5000:{BDS_CONTAINER_HTTP_PORT}/tcp"),
            format!(
                "127.0.0.1:{}:{}/udp",
                udp_port_range(5000, players),
                udp_port_range(BDS_CONTAINER_UDP_PORT, players)
            ),
            format!(
                "SERVER_UDP_PORTS={}",
                bds_udp_mapping(5000, BDS_CONTAINER_UDP_PORT, players)
            ),
            format!("MAX_PLAYERS={players}"),
            "TRANSPORT=nethernet".to_owned(),
            "ONLINE_MODE=false".to_owned(),
        ] {
            assert!(args.contains(&expected), "{expected} missing from {args:?}");
        }
        let health = args
            .iter()
            .position(|arg| arg == "--health-cmd")
            .expect("health check");
        assert!(
            args[health + 1].contains("/v1/join"),
            "health must check HTTP status"
        );
    }
}

/// LAN discovery is explicit, and the container publishes it on loopback only.
#[test]
fn lan_discovery_is_explicit_and_loopback_only() {
    let dir = scratch();
    let spec = spec(dir.path());
    let default_mapping = format!("127.0.0.1:{LAN_DISCOVERY_PORT}:{LAN_DISCOVERY_PORT}/udp");
    for visible in [false, true] {
        let args = args(&spec, 1, visible, 0);
        assert_eq!(args.contains(&default_mapping), visible, "{args:?}");
        assert!(args.contains(&format!("ENABLE_LAN_VISIBILITY={visible}")));
    }
    let alternate = args(&spec, 8, true, LAN_DISCOVERY_PORT + 1);
    let mapping = format!(
        "127.0.0.1:{}:{LAN_DISCOVERY_PORT}/udp",
        LAN_DISCOVERY_PORT + 1
    );
    assert!(alternate.contains(&mapping), "{alternate:?}");
}
