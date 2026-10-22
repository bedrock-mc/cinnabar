use bridge::GameMode;
use launcher::local_worlds::world::Difficulty;

use super::*;
#[cfg(unix)]
use crate::local_worlds::test_support::{FAKE_BDS, installed_provisioner};
use crate::local_worlds::{
    provision::Platform,
    test_support::{VERSION, scratch, spec},
};

const LINUX: Platform = Platform {
    os: "linux",
    arch: "amd64",
};

#[test]
fn server_properties_configure_local_play() {
    let dir = scratch();
    let mut spec = spec(dir.path());
    spec.world.name = "My\nWorld #1".into();
    spec.world.game_mode = GameMode::Creative;
    spec.world.difficulty = Difficulty::Hard;
    spec.world.seed = -5;
    let properties = server_properties(&spec, 5000, 1, false);
    for expected in [
        "server-name=MyWorld 1\n",
        "gamemode=creative\n",
        "force-gamemode=true\n",
        "difficulty=hard\n",
        "online-mode=false\n",
        "allow-list=false\n",
        "max-players=1\n",
        "server-port=5000\n",
        "server-udp-ports=127.0.0.1:5000:5000\n",
        "level-name=0123456789abcdef\n",
        "level-seed=-5\n",
        "level-type=FLAT\n",
        "tick-distance=4\n",
        "transport=nethernet\n",
        "server-ip=127.0.0.1\n",
        "enable-lan-visibility=false\n",
    ] {
        assert!(
            properties.contains(expected),
            "missing {expected:?}:\n{properties}"
        );
    }
    spec.world.generator = Generator::Normal;
    let defaults = server_properties(&spec, 1, 1, true);
    for expected in ["level-type=DEFAULT\n", "enable-lan-visibility=true\n"] {
        assert!(
            defaults.contains(expected),
            "missing {expected:?}:\n{defaults}"
        );
    }
    assert!(
        !defaults.contains("server-portv6="),
        "NetherNet ignores the separate IPv6 port"
    );
}

/// BDS's view distance only caps RequestChunkRadius, so it must not hold the client below its
/// render distance; the simulation distance follows the world option however far the client sees.
#[test]
fn the_view_distance_admits_every_client_render_distance() {
    let dir = scratch();
    let mut spec = spec(dir.path());
    let max = render_api::MAX_VIEW_RADIUS_CHUNKS;
    for (requested, view) in [(None, max), (Some(64), 64), (Some(2), 5)] {
        spec.view_distance = requested.map(i32::unsigned_abs);
        let properties = server_properties(&spec, 5000, 1, false);
        for expected in [
            format!("view-distance={view}\n"),
            "tick-distance=4\n".to_owned(),
        ] {
            assert!(
                properties.contains(&expected),
                "missing {expected:?}:\n{properties}"
            );
        }
    }
}

/// Shared worlds keep their player limit and size the UDP window to it.
#[test]
fn the_player_limit_sizes_the_udp_window() {
    let dir = scratch();
    for players in [8, 16] {
        let properties = server_properties(&spec(dir.path()), 5000, players, false);
        assert!(
            properties.contains(&format!("max-players={players}\n")),
            "{properties}"
        );
        let window = format!(
            "server-udp-ports={}\n",
            bds_udp_mapping(5000, 5000, players)
        );
        assert!(properties.contains(&window), "{properties}");
    }
    assert_eq!(HOSTED_WORLD_MAX_PLAYERS, 8);
}

#[test]
fn the_native_runner_needs_the_eula() {
    let dir = scratch();
    let mut provisioner = Provisioner::new(dir.path().join("bds"), VERSION.into());
    provisioner.platform = LINUX;
    let runner = BdsRunner::new(Arc::new(provisioner), String::new());
    let started = runner.start(&spec(dir.path()), &Cancel::never());
    assert!(matches!(started, Err(WorldError::EulaRequired)));
}

/// The world folder is linked into the install while the server runs, and only the link goes.
#[cfg(unix)]
#[test]
fn the_native_runner_links_the_world_and_cleans_up_on_stop() {
    let dir = scratch();
    let provisioner = installed_provisioner(dir.path(), LINUX, FAKE_BDS);
    let install = provisioner.root.join(VERSION);
    let runner = BdsRunner::new(Arc::new(provisioner), String::new());
    let world_dir = dir.path().join("world");
    let spec = spec(&world_dir);
    let instance = runner.start(&spec, &Cancel::never()).expect("start");
    let level = fs::read(world_dir.join(LEVEL_DIR).join("level.dat")).expect("level data");
    let has = |tag: &[u8]| level.windows(tag.len()).any(|window| window == tag);
    assert!(
        has(b"Created") && has(b"showcoordinates"),
        "BDS creates the world before owned fields are patched"
    );
    let properties = fs::read_to_string(install.join("server.properties")).expect("properties");
    assert!(properties.contains("online-mode=false"), "{properties}");
    let link = install.join("worlds").join(&spec.world.id);
    assert_eq!(
        fs::canonicalize(&link).expect("link"),
        fs::canonicalize(world_dir.join(LEVEL_DIR)).expect("world data")
    );
    assert!(!instance.can_pause(), "BDS cannot pause");
    instance
        .set_paused(true)
        .expect("pausing is a harmless no-op");
    assert_eq!(instance.max_players(), Some(HOSTED_WORLD_MAX_PLAYERS));
    instance.stop(test_time::DEFAULT_TIMEOUT).expect("stop");
    assert!(
        fs::symlink_metadata(link).is_err(),
        "the world link survived the stop"
    );
    assert!(
        world_dir.join(LEVEL_DIR).is_dir(),
        "world data must survive the stop"
    );
}

/// The host is the only operator, and a later start without one drops the earlier grant.
#[cfg(unix)]
#[test]
fn the_native_runner_makes_only_the_host_an_operator() {
    let dir = scratch();
    let provisioner = installed_provisioner(dir.path(), LINUX, FAKE_BDS);
    let install = provisioner.root.join(VERSION);
    let runner = BdsRunner::new(Arc::new(provisioner), String::new());
    let world_dir = dir.path().join("world");
    let operators = || {
        let file = fs::read(install.join(PERMISSIONS_FILE)).expect("permissions");
        serde_json::from_slice::<serde_json::Value>(&file).expect("permissions JSON")
    };
    let mut spec = spec(&world_dir);
    spec.host_xuid = Some("2535400000000001".to_owned());
    let instance = runner.start(&spec, &Cancel::never()).expect("start");
    assert_eq!(
        operators(),
        serde_json::json!([{ "permission": "operator", "xuid": "2535400000000001" }])
    );
    let properties = fs::read_to_string(install.join("server.properties")).expect("properties");
    assert!(
        properties.contains("default-player-permission-level=member"),
        "other players keep the world's default: {properties}"
    );
    instance.stop(test_time::DEFAULT_TIMEOUT).expect("stop");
    spec.host_xuid = None;
    let instance = runner.start(&spec, &Cancel::never()).expect("restart");
    assert_eq!(operators(), serde_json::json!([]));
    instance.stop(test_time::DEFAULT_TIMEOUT).expect("stop");
}

/// A first start that saves no world fails clearly instead of writing level data itself.
#[cfg(unix)]
#[test]
fn a_new_world_bds_did_not_save_fails_without_writing_level_data() {
    let dir = scratch();
    let server = b"#!/bin/sh\nprintf '[INFO] Server started.\\r\\n'\nwhile IFS= read -r line; do\n  case \"$line\" in stop*) exit 0 ;; esac\ndone\n";
    let provisioner = installed_provisioner(dir.path(), LINUX, server);
    let runner = BdsRunner::new(Arc::new(provisioner), String::new());
    let world_dir = dir.path().join("world");
    let error = runner
        .start(&spec(&world_dir), &Cancel::never())
        .err()
        .expect("no world was saved");
    assert!(error.to_string().contains("no level data"), "{error}");
    assert!(!world_dir.join(LEVEL_DIR).join("level.dat").exists());
}

#[cfg(unix)]
#[test]
fn a_failed_start_removes_the_world_link() {
    let dir = scratch();
    let provisioner = installed_provisioner(dir.path(), LINUX, b"#!/bin/sh\nexit 3\n");
    let install = provisioner.root.join(VERSION);
    let runner = BdsRunner::new(Arc::new(provisioner), String::new());
    let spec = spec(&dir.path().join("world"));
    assert!(runner.start(&spec, &Cancel::never()).is_err());
    let link = install.join("worlds").join(&spec.world.id);
    assert!(
        fs::symlink_metadata(link).is_err(),
        "the link outlived the failed start"
    );
}

#[test]
fn world_options_properties_match_native_and_container_configuration() {
    use crate::local_worlds::container::{ContainerArgs, container_args};
    use launcher::local_worlds::options::{Control, Storage};
    let dir = scratch();
    let mut spec = spec(dir.path());
    for &id in O::ALL {
        let definition = id.definition();
        let Storage::Property(key, _) = definition.storage else {
            continue;
        };
        let value = match definition.control {
            Control::Toggle => 1 - definition.default,
            Control::Choice(choices) => {
                choices
                    .iter()
                    .find(|choice| choice.value != definition.default)
                    .unwrap()
                    .value
            }
            Control::Number { max, .. } => max,
        };
        spec.world.options.set(id, value);
        let text = match definition.control {
            Control::Toggle => (value != 0).to_string(),
            Control::Number { .. } => value.to_string(),
            Control::Choice(choices) => choices
                .iter()
                .find(|choice| choice.value == value)
                .unwrap()
                .wire
                .to_owned(),
        };
        let native = server_properties(&spec, 5000, 8, false);
        assert!(
            native.lines().any(|line| line == format!("{key}={text}")),
            "{id:?}: {native}"
        );
        let container = container_args(&ContainerArgs {
            spec: &spec,
            image: "fixture",
            version: VERSION,
            install: dir.path(),
            host_port: 5000,
            max_players: 8,
            lan_visible: false,
            lan_host_port: 0,
        });
        let environment = format!("{}={text}", key.replace('-', "_").to_ascii_uppercase());
        assert!(
            container.iter().any(|value| value == &environment),
            "{id:?}"
        );
    }
    spec.world.options.set(O::Multiplayer, 0);
    assert_eq!(world_player_limit(&spec, 8), 1);
    spec.world.options.set(O::Multiplayer, 1);
    assert_eq!(world_player_limit(&spec, 8), 8);
}
