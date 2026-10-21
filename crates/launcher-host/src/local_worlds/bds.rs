use std::{
    ffi::OsString,
    fmt::Write as _,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Once},
    thread,
    time::Duration,
};

use bridge::contract::HOSTED_WORLD_MAX_PLAYERS;
use crossbeam_channel::Receiver;
use launcher::local_worlds::{
    options::{Storage, WorldOption as O},
    world::Generator,
};

use super::{
    Cancel, create_private_dir,
    docker::DOCKER,
    junction::create_world_link,
    ports::{LOCAL_SERVER_HOST, bds_udp_mapping, free_bds_port, lan_address, loopback},
    process::{Instance, Launch, Process, Runner, StartSpec, launch},
    provision::{Provisioner, RuntimeKind, install_dir},
    store::LEVEL_DIR,
    world::{WorldError, failed, wire_name},
    write_private,
};

const BDS_START_TIMEOUT: Duration = Duration::from_secs(5 * 60);
pub(crate) const BDS_READY_MARKER: &str = "Server started.";
pub(crate) const DEFAULT_BDS_VIEW: u32 = 10;
/// BDS's `transport` setting for its NetherNet HTTP listener.
pub(crate) const BDS_TRANSPORT: &str = "nethernet";
/// Makes existing BDS worlds use the launcher's selected game mode at login.
pub(crate) const FORCE_GAME_MODE: bool = true;

/// Hosts each world in the official Bedrock Dedicated Server, natively or in a container.
///
/// The NetherNet HTTP listener binds to loopback with online mode off; its UDP window is sized to
/// `max_players`. The world folder is exposed to the server through a directory link at
/// `worlds/<id>` inside the install, so only one world runs at a time.
pub(crate) struct BdsRunner {
    pub provisioner: Arc<Provisioner>,
    /// Added to the inherited environment.
    pub env: Vec<(OsString, OsString)>,
    /// `None` takes the runtime's default.
    pub start_timeout: Option<Duration>,
    pub max_players: u32,
    /// Zero selects an available loopback TCP and UDP window.
    pub host_port: u16,
    /// Container-only discovery host port; zero uses the default discovery port.
    pub lan_host_port: u16,
    /// The Docker-compatible CLI of the container runtime.
    pub docker: String,
    /// Must name a digest.
    pub image: String,
}

impl BdsRunner {
    pub(crate) fn new(provisioner: Arc<Provisioner>, image: String) -> Self {
        Self {
            provisioner,
            env: Vec::new(),
            start_timeout: None,
            max_players: HOSTED_WORLD_MAX_PLAYERS,
            host_port: 0,
            lan_host_port: 0,
            docker: DOCKER.to_owned(),
            image,
        }
    }
}

impl Runner for BdsRunner {
    fn start(&self, spec: &StartSpec, cancel: &Cancel) -> Result<Arc<dyn Instance>, WorldError> {
        if self.provisioner.runtime_info().kind == RuntimeKind::Container {
            return self.start_container(spec, cancel);
        }
        let binary = self.provisioner.ensure(cancel)?;
        let max_players = world_player_limit(spec, self.max_players);
        let lan_visible = spec.world.options.lan_visible();
        let port = free_bds_port(max_players, self.host_port)?;
        let install = install_dir(&binary)?;
        let world_dir = spec.dir.join(LEVEL_DIR);
        create_private_dir(&world_dir)
            .map_err(|error| failed(format!("create world folder: {error}")))?;
        super::level_dat::configure(&spec.world, &world_dir)?;
        let link = install.join("worlds").join(&spec.world.id);
        link_world(&link, &world_dir)?;
        let properties = server_properties(spec, port, max_players, lan_visible);
        if let Err(error) = write_private(&install.join("server.properties"), properties.as_bytes())
        {
            unlink_world(&link);
            return Err(failed(format!("write server.properties: {error}")));
        }
        let mut command = Command::new(binary.as_os_str());
        command
            .current_dir(install)
            .env("LD_LIBRARY_PATH", ".")
            .envs(self.env.iter().map(|(key, value)| (key, value)));
        let launched = launch(
            command,
            Launch {
                address: loopback(port),
                timeout: self.start_timeout.unwrap_or(BDS_START_TIMEOUT),
                can_pause: false,
                ready: |line| line.contains(BDS_READY_MARKER),
                label: format!("bds {}", spec.world.id),
            },
            cancel,
        );
        let process = match launched {
            Ok(process) => process,
            Err(error) => {
                unlink_world(&link);
                return Err(error);
            }
        };
        let instance = Arc::new(BdsInstance {
            process,
            link,
            unlinked: Once::new(),
            lan_address: lan_visible.then(|| lan_address(0)),
            max_players,
        });
        let watched = Arc::clone(&instance);
        thread::spawn(move || {
            let _ = watched.process.exited().recv();
            watched.unlink();
        });
        let instance: Arc<dyn Instance> = instance;
        Ok(instance)
    }
}

/// A native server that drops its world link once it has exited.
struct BdsInstance {
    process: Arc<Process>,
    link: PathBuf,
    unlinked: Once,
    lan_address: Option<String>,
    max_players: u32,
}

impl BdsInstance {
    fn unlink(&self) {
        self.unlinked.call_once(|| unlink_world(&self.link));
    }
}

impl Instance for BdsInstance {
    fn address(&self) -> &str {
        self.process.address()
    }

    fn set_paused(&self, _paused: bool) -> io::Result<()> {
        Ok(())
    }

    fn request_stop(&self) {
        self.process.request_stop();
    }

    fn stop(&self, timeout: Duration) -> Result<(), String> {
        let stopped = self.process.stop(timeout);
        self.unlink();
        stopped
    }

    fn exited(&self) -> Receiver<()> {
        self.process.exited()
    }

    /// BDS registers no sim-time pause (`/globalpause`), gamerules leave mobs and redstone running,
    /// and suspending the process would also freeze the connection.
    fn can_pause(&self) -> bool {
        false
    }

    fn max_players(&self) -> Option<u32> {
        Some(self.max_players)
    }

    fn lan_address(&self) -> Option<&str> {
        self.lan_address.as_deref()
    }
}

/// The view distance BDS gets, and the tick distance that follows from it.
pub(crate) fn view_and_tick(spec: &StartSpec) -> (u32, u32) {
    let view = spec
        .view_distance
        .filter(|view| *view > 0)
        .unwrap_or(DEFAULT_BDS_VIEW)
        .clamp(5, 32);
    (view, spec.world.options.get(O::SimulationDistance) as u32)
}

/// BDS's `level-type` for a generator.
pub(crate) fn level_type(generator: Generator) -> &'static str {
    match generator {
        Generator::Flat => "FLAT",
        Generator::Normal => "DEFAULT",
    }
}

/// The per-launch BDS configuration. Seed, level type and the initial settings apply only when
/// the world is first created; game mode and difficulty apply on every start.
pub(crate) fn server_properties(
    spec: &StartSpec,
    port: u16,
    max_players: u32,
    lan_visible: bool,
) -> String {
    let world = &spec.world;
    let (view, _) = view_and_tick(spec);
    let mut properties = String::new();
    for (key, value) in [
        ("server-name", sanitize_property(&world.name)),
        ("gamemode", wire_name(world.game_mode)),
        ("force-gamemode", FORCE_GAME_MODE.to_string()),
        ("difficulty", wire_name(world.difficulty)),
        ("max-players", max_players.to_string()),
        ("online-mode", "false".to_owned()),
        ("allow-list", "false".to_owned()),
        ("server-port", port.to_string()),
        ("server-ip", LOCAL_SERVER_HOST.to_owned()),
        ("server-udp-ports", bds_udp_mapping(port, port, max_players)),
        ("level-name", world.id.clone()),
        ("level-seed", world.seed.to_string()),
        ("level-type", level_type(world.generator).to_owned()),
        ("view-distance", view.to_string()),
        ("player-idle-timeout", "0".to_owned()),
        ("transport", BDS_TRANSPORT.to_owned()),
        ("texturepack-required", "false".to_owned()),
        ("content-log-file-enabled", "false".to_owned()),
    ] {
        let _ = writeln!(properties, "{key}={value}");
    }
    for (key, value) in option_properties(spec, lan_visible) {
        let _ = writeln!(properties, "{key}={value}");
    }
    properties
}

/// Limits a single-player world to its host while keeping multiplayer's configured capacity.
pub(crate) fn world_player_limit(spec: &StartSpec, configured: u32) -> u32 {
    if spec.world.options.enabled(O::Multiplayer) {
        configured.max(1)
    } else {
        1
    }
}

/// Generates the shared option properties used by native and container BDS.
pub(crate) fn option_properties(
    spec: &StartSpec,
    lan_visible: bool,
) -> Vec<(&'static str, String)> {
    O::ALL
        .iter()
        .filter_map(|&id| {
            let definition = id.definition();
            let Storage::Property(key, _) = definition.storage else {
                return None;
            };
            let options = &spec.world.options;
            let value = match definition.control {
                launcher::local_worlds::options::Control::Toggle => (if id == O::LanVisible {
                    lan_visible && options.lan_visible()
                } else {
                    options.enabled(id)
                })
                .to_string(),
                launcher::local_worlds::options::Control::Choice(choices) => choices
                    .iter()
                    .find(|choice| choice.value == options.get(id))
                    .expect("validated world option")
                    .wire
                    .to_owned(),
                launcher::local_worlds::options::Control::Number { .. } => {
                    options.get(id).to_string()
                }
            };
            Some((key, value))
        })
        .collect()
}

/// Drops control characters and `#`, which would end or comment out a property line.
pub(crate) fn sanitize_property(value: &str) -> String {
    value
        .chars()
        .filter(|&character| character >= ' ' && character != '\u{7f}' && character != '#')
        .collect()
}

/// Exposes `target` at `link`: a junction on Windows, which needs no privilege; a symlink elsewhere.
pub(crate) fn link_world(link: &Path, target: &Path) -> Result<(), WorldError> {
    if let Some(parent) = link.parent() {
        create_private_dir(parent)
            .map_err(|error| failed(format!("prepare worlds folder: {error}")))?;
    }
    unlink_world(link);
    create_world_link(link, target).map_err(|error| failed(format!("link world folder: {error}")))
}

/// Removes only the link, never the world data it points at; a real folder goes only when empty.
pub(crate) fn unlink_world(link: &Path) {
    let Ok(metadata) = fs::symlink_metadata(link) else {
        return;
    };
    let kind = metadata.file_type();
    if kind.is_symlink() {
        // Windows removes junctions and directory symlinks as folders, without following them.
        let _ = if cfg!(windows) {
            fs::remove_dir(link)
        } else {
            fs::remove_file(link)
        };
    } else if kind.is_dir() {
        let _ = fs::remove_dir(link);
    }
}

/// Drops world links and empty mount points from an install; false when anything else remains,
/// so deleting the install can never reach world data.
pub(crate) fn release_world_links(install: &Path) -> bool {
    let worlds = install.join("worlds");
    let entries = match fs::read_dir(&worlds) {
        Ok(entries) => entries,
        Err(error) => return error.kind() == io::ErrorKind::NotFound,
    };
    for entry in entries.flatten() {
        unlink_world(&entry.path());
    }
    fs::read_dir(worlds).is_ok_and(|mut left| left.next().is_none())
}

#[cfg(test)]
#[path = "bds_tests.rs"]
mod tests;
