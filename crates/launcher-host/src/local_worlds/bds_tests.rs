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
    spec.view_distance = Some(64);
    let properties = server_properties(&spec, 5000, 1, false);
    for expected in [
        "server-name=MyWorld 1\n",
        "gamemode=creative\n",
        "difficulty=hard\n",
        "online-mode=false\n",
        "allow-list=false\n",
        "max-players=1\n",
        "server-port=5000\n",
        "server-udp-ports=127.0.0.1:5000:5000\n",
        "level-name=0123456789abcdef\n",
        "level-seed=-5\n",
        "level-type=FLAT\n",
        "view-distance=32\n",
        "tick-distance=12\n",
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
    spec.view_distance = None;
    let defaults = server_properties(&spec, 1, 1, true);
    for expected in [
        "level-type=DEFAULT\n",
        "view-distance=10\n",
        "enable-lan-visibility=true\n",
    ] {
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
    let properties = fs::read_to_string(install.join("server.properties")).expect("properties");
    assert!(properties.contains("online-mode=false"), "{properties}");
    let link = install.join("worlds").join(&spec.world.id);
    assert_eq!(
        fs::canonicalize(&link).expect("link"),
        fs::canonicalize(world_dir.join("db")).expect("world data")
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
        world_dir.join("db").is_dir(),
        "world data must survive the stop"
    );
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
