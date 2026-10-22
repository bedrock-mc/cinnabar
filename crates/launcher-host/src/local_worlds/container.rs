use std::{
    collections::HashMap,
    ffi::OsString,
    fs,
    io::{BufRead, BufReader, Read},
    path::Path,
    process::{Command, Stdio},
    sync::{Arc, Once},
    thread,
    time::Duration,
};

use crossbeam_channel::Receiver;

use crate::lifecycle::children;
use launcher::local_worlds::options::WorldOption;
use launcher::local_worlds::world::{SetupState, UnavailableReason};

use super::{
    Cancel,
    bds::{
        BDS_READY_MARKER, BDS_TRANSPORT, BdsRunner, level_type, option_properties,
        sanitize_property, view_and_tick, world_player_limit,
    },
    create_private_dir,
    docker::{PROBE_TIMEOUT, docker_command, finishes, succeeds},
    download::direct_url,
    ports::{
        BDS_CONTAINER_HTTP_PORT, BDS_CONTAINER_UDP_PORT, LAN_DISCOVERY_PORT, LOCAL_SERVER_HOST,
        bds_udp_mapping, check_bds_lan_port, free_bds_port, lan_address, loopback, udp_port_range,
    },
    process::{Instance, Launch, Process, StartSpec, launch},
    provision::{RuntimeInfo, install_dir},
    store::LEVEL_DIR,
    world::{WorldError, failed, wire_name},
};

/// The first start of a world generates spawn chunks under emulation.
const CONTAINER_START_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Inside the manager's 30 s stop budget.
const CONTAINER_STOP_SECONDS: u32 = 25;
const CONTAINER_PLATFORM: &str = "linux/amd64";
/// Bounds Docker housekeeping calls (inspect, remove).
const DOCKER_TIMEOUT: Duration = Duration::from_secs(120);

/// The container that hosts `world_id`.
pub(crate) fn container_name(world_id: &str) -> String {
    format!("cinnabar-bds-{world_id}")
}

/// What `docker run` needs beyond the world.
pub(crate) struct ContainerArgs<'a> {
    pub spec: &'a StartSpec,
    pub image: &'a str,
    pub version: &'a str,
    /// The host-provisioned install, mounted as `/data`.
    pub install: &'a Path,
    pub host_port: u16,
    pub max_players: u32,
    pub lan_visible: bool,
    /// Zero uses the default discovery port.
    pub lan_host_port: u16,
}

/// `docker run`: loopback-only HTTP and UDP mappings, the install as `/data` (the image skips its
/// own download when `bedrock_server-<version>` is present), the world folder bind-mounted into the
/// server's worlds directory, and the EULA acknowledged (callers check acceptance first).
pub(crate) fn container_args(args: &ContainerArgs<'_>) -> Vec<String> {
    let world = &args.spec.world;
    let (view, _) = view_and_tick(args.spec);
    let players = args.max_players;
    let mut run = vec![
        "run".to_owned(),
        "--rm".to_owned(),
        "--name".to_owned(),
        container_name(&world.id),
        "--platform".to_owned(),
        CONTAINER_PLATFORM.to_owned(),
        "-p".to_owned(),
        format!(
            "{LOCAL_SERVER_HOST}:{}:{BDS_CONTAINER_HTTP_PORT}/tcp",
            args.host_port
        ),
        "-p".to_owned(),
        format!(
            "{LOCAL_SERVER_HOST}:{}:{}/udp",
            udp_port_range(args.host_port, players),
            udp_port_range(BDS_CONTAINER_UDP_PORT, players)
        ),
        "--health-cmd".to_owned(),
        format!(
            "curl --fail --silent http://{LOCAL_SERVER_HOST}:{BDS_CONTAINER_HTTP_PORT}/v1/join > /dev/null"
        ),
        "-v".to_owned(),
        format!("{}:/data", args.install.display()),
        "-v".to_owned(),
        format!(
            "{}:/data/worlds/{}",
            args.spec.dir.join(LEVEL_DIR).display(),
            world.id
        ),
    ];
    if args.lan_visible {
        let host = if args.lan_host_port == 0 {
            LAN_DISCOVERY_PORT
        } else {
            args.lan_host_port
        };
        run.push("-p".to_owned());
        run.push(format!(
            "{LOCAL_SERVER_HOST}:{host}:{LAN_DISCOVERY_PORT}/udp"
        ));
    }
    for (key, value) in [
        ("EULA", "TRUE".to_owned()),
        ("VERSION", args.version.to_owned()),
        ("DIRECT_DOWNLOAD_URL", direct_url("linux", args.version)),
        ("SERVER_NAME", sanitize_property(&world.name)),
        ("GAMEMODE", wire_name(world.game_mode)),
        ("FORCE_GAMEMODE", super::bds::FORCE_GAME_MODE.to_string()),
        ("DIFFICULTY", wire_name(world.difficulty)),
        ("MAX_PLAYERS", players.to_string()),
        ("ONLINE_MODE", "false".to_owned()),
        ("ALLOW_LIST", "false".to_owned()),
        ("LEVEL_NAME", world.id.clone()),
        ("LEVEL_SEED", world.seed.to_string()),
        ("LEVEL_TYPE", level_type(world.generator).to_owned()),
        ("VIEW_DISTANCE", view.to_string()),
        ("PLAYER_IDLE_TIMEOUT", "0".to_owned()),
        ("TRANSPORT", BDS_TRANSPORT.to_owned()),
        ("SERVER_PORT", BDS_CONTAINER_HTTP_PORT.to_string()),
        ("SERVER_IP", "0.0.0.0".to_owned()),
        (
            "SERVER_UDP_PORTS",
            bds_udp_mapping(args.host_port, BDS_CONTAINER_UDP_PORT, players),
        ),
        // The image's shim for IPv6 binds Docker cannot serve.
        ("ENABLE_BDS_V6BIND_FIX", "TRUE".to_owned()),
    ] {
        run.push("-e".to_owned());
        run.push(format!("{key}={value}"));
    }
    for (key, value) in option_properties(args.spec, args.lan_visible) {
        run.push("-e".to_owned());
        run.push(format!(
            "{}={value}",
            key.replace('-', "_").to_ascii_uppercase()
        ));
    }
    run.push(args.image.to_owned());
    run
}

/// Counts image layers in `docker pull` output.
#[derive(Default)]
pub(crate) struct PullProgress {
    /// Layer id to whether it is complete.
    layers: HashMap<String, bool>,
    done: u32,
}

impl PullProgress {
    /// Reads one output line and returns the completed and total layers so far.
    pub(crate) fn line(&mut self, line: &str) -> (u32, u32) {
        if let Some((id, status)) = line.trim().split_once(": ")
            && id.len() == 12
            && id
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
            && !status.is_empty()
        {
            let complete = status == "Pull complete" || status == "Already exists";
            let was = self.layers.get(id).copied();
            if was.is_none() || (complete && was == Some(false)) {
                self.layers.insert(id.to_owned(), complete);
                if complete {
                    self.done += 1;
                }
            }
        }
        (
            self.done,
            u32::try_from(self.layers.len()).unwrap_or(u32::MAX),
        )
    }
}

impl BdsRunner {
    fn docker(&self) -> Command {
        docker_command(&self.docker, &self.env)
    }

    fn docker_succeeds(&self, args: &[&str], timeout: Duration) -> bool {
        let mut command = self.docker();
        command.args(args);
        succeeds(command, timeout)
    }

    /// Runs a startup Docker call that the start's cancellation ends, as a closed world needs.
    fn docker_until(
        &self,
        args: &[&str],
        timeout: Duration,
        cancel: &Cancel,
    ) -> Result<bool, WorldError> {
        let mut command = self.docker();
        command.args(args);
        finishes(command, timeout, cancel).ok_or(WorldError::Cancelled)
    }

    /// Runs the Linux build in a container; the open-time probe flips the runtime when Docker has
    /// stopped so the client offers Retry.
    pub(super) fn start_container(
        &self,
        spec: &StartSpec,
        cancel: &Cancel,
    ) -> Result<Arc<dyn Instance>, WorldError> {
        let provisioner = &self.provisioner;
        if !provisioner.eula_accepted() {
            return Err(WorldError::EulaRequired);
        }
        if !self.image.contains("@sha256:") {
            return Err(WorldError::ImageNotPinned);
        }
        let max_players = world_player_limit(spec, self.max_players);
        // Ports are published on loopback only, so other devices could never join from LAN.
        let lan_visible = false;
        self.check_docker(cancel)?;
        self.ensure_image(cancel)?;
        let binary = provisioner.ensure(cancel)?;
        let install = install_dir(&binary)?;
        let version = install
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        link_versioned_binary(&binary, &version)?;
        create_private_dir(&spec.dir.join(LEVEL_DIR))
            .map_err(|error| failed(format!("create world folder: {error}")))?;
        let name = container_name(&spec.world.id);
        // A leftover of an earlier crash would hold the name and its ports.
        self.docker_until(&["rm", "-f", name.as_str()], DOCKER_TIMEOUT, cancel)?;
        let port = free_bds_port(max_players, self.host_port)?;
        if lan_visible {
            let lan_port = if self.lan_host_port == 0 {
                LAN_DISCOVERY_PORT
            } else {
                self.lan_host_port
            };
            check_bds_lan_port(port, max_players, lan_port)?;
        }
        let mut world = spec.world.clone();
        world.options.set(WorldOption::LanVisible, 0);
        super::level_dat::configure(&world, &spec.dir.join(LEVEL_DIR))?;
        let mut command = self.docker();
        command.args(container_args(&ContainerArgs {
            spec,
            image: &self.image,
            version: &version,
            install,
            host_port: port,
            max_players,
            lan_visible,
            lan_host_port: self.lan_host_port,
        }));
        let launched = launch(
            command,
            Launch {
                address: loopback(port),
                timeout: self.start_timeout.unwrap_or(CONTAINER_START_TIMEOUT),
                can_pause: false,
                ready: |line| line.contains(BDS_READY_MARKER),
                label: format!("bds-container {}", spec.world.id),
            },
            cancel,
        );
        let process = match launched {
            Ok(process) => process,
            Err(error) => {
                self.docker_succeeds(&["rm", "-f", name.as_str()], DOCKER_TIMEOUT);
                return Err(error);
            }
        };
        let instance = Arc::new(ContainerInstance {
            process,
            docker: self.docker.clone(),
            env: self.env.clone(),
            name,
            removed: Once::new(),
            max_players,
            lan_address: lan_visible.then(|| lan_address(self.lan_host_port)),
        });
        let watched = Arc::clone(&instance);
        thread::spawn(move || {
            let _ = watched.process.exited().recv();
            watched.remove();
        });
        let instance: Arc<dyn Instance> = instance;
        Ok(instance)
    }

    fn check_docker(&self, cancel: &Cancel) -> Result<(), WorldError> {
        let provisioner = &self.provisioner;
        provisioner.set_op(Some(SetupState::CheckingRuntime), None, 0, 0);
        let running = self.docker_until(&["info"], PROBE_TIMEOUT, cancel);
        let Ok(running) = running else {
            provisioner.set_op(None, None, 0, 0);
            return Err(WorldError::Cancelled);
        };
        if !running {
            provisioner.set_runtime(RuntimeInfo::none(
                "Docker stopped; new worlds use dragonfly",
                Some(UnavailableReason::DockerNotRunning),
            ));
        }
        provisioner.set_op(None, None, 0, 0);
        if running {
            Ok(())
        } else {
            Err(WorldError::DockerNotRunning)
        }
    }

    /// Pulls the image once, reporting layer progress.
    fn ensure_image(&self, cancel: &Cancel) -> Result<(), WorldError> {
        if self.docker_until(
            &["image", "inspect", self.image.as_str()],
            DOCKER_TIMEOUT,
            cancel,
        )? {
            return Ok(());
        }
        let provisioner = &self.provisioner;
        provisioner.set_op(Some(SetupState::PullingImage), None, 0, 0);
        let pull_failed = |detail: String| {
            provisioner.fail_with(
                "server image download failed",
                failed(format!("pull server image: {detail}")),
            )
        };
        let mut command = self.docker();
        command
            .args([
                "pull",
                "--platform",
                CONTAINER_PLATFORM,
                self.image.as_str(),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // Tracked, so an exit sweep ends a pull the start's cancellation has not reached yet.
        let child =
            children::spawn(&mut command).map_err(|error| pull_failed(error.to_string()))?;
        let (stdout, stderr) = (child.take_stdout(), child.take_stderr());
        let child = Arc::new(child);
        let errors = thread::spawn(move || {
            let mut text = String::new();
            if let Some(mut stderr) = stderr {
                let _ = stderr.read_to_string(&mut text);
            }
            text
        });
        let (pulled, done) = crossbeam_channel::bounded::<()>(0);
        let watcher = {
            let (child, cancelled) = (Arc::clone(&child), cancel.receiver().clone());
            thread::spawn(move || {
                crossbeam_channel::select! {
                    recv(cancelled) -> _ => child.kill(),
                    recv(done) -> _ => {}
                }
            })
        };
        let mut progress = PullProgress::default();
        if let Some(stdout) = stdout {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let (done, total) = progress.line(&line);
                provisioner.set_layers(done, total);
            }
        }
        let status = child.wait();
        drop(pulled);
        let _ = watcher.join();
        let errors = errors.join().unwrap_or_default();
        match status {
            Some(status) if status.success() => {
                provisioner.set_op(None, None, 0, 0);
                Ok(())
            }
            _ if cancel.is_cancelled() => Err(WorldError::Cancelled),
            Some(status) => {
                tracing::error!("docker pull failed: {}", errors.trim());
                Err(pull_failed(status.to_string()))
            }
            None => Err(pull_failed("the pull process was lost".to_owned())),
        }
    }
}

/// Names the server binary as the image expects, so it never downloads its own copy.
fn link_versioned_binary(binary: &Path, version: &str) -> Result<(), WorldError> {
    let mut versioned = binary.as_os_str().to_owned();
    versioned.push(format!("-{version}"));
    if fs::metadata(&versioned).is_ok() {
        return Ok(());
    }
    fs::hard_link(binary, versioned)
        .map_err(|error| failed(format!("prepare server binary: {error}")))
}

/// Stops the container gracefully (BDS saves on SIGTERM) and always removes it.
struct ContainerInstance {
    process: Arc<Process>,
    docker: String,
    env: Vec<(OsString, OsString)>,
    name: String,
    removed: Once,
    max_players: u32,
    lan_address: Option<String>,
}

impl ContainerInstance {
    fn command(&self, args: &[&str]) -> Command {
        let mut command = docker_command(&self.docker, &self.env);
        command.args(args);
        command
    }

    fn stop_command(&self) -> Command {
        let seconds = CONTAINER_STOP_SECONDS.to_string();
        self.command(&["stop", "-t", seconds.as_str(), self.name.as_str()])
    }

    fn remove(&self) {
        self.removed.call_once(|| {
            succeeds(
                self.command(&["rm", "-f", self.name.as_str()]),
                DOCKER_TIMEOUT,
            );
        });
    }
}

impl Instance for ContainerInstance {
    fn address(&self) -> &str {
        self.process.address()
    }

    fn set_paused(&self, _paused: bool) -> std::io::Result<()> {
        Ok(())
    }

    /// Starts `docker stop` at once, untracked, so it still stops the container when the client
    /// exits right after.
    fn request_stop(&self) {
        let mut stop = self.stop_command();
        stop.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Ok(mut stopping) = stop.spawn() {
            thread::spawn(move || stopping.wait());
        }
    }

    fn stop(&self, timeout: Duration) -> Result<(), String> {
        succeeds(self.stop_command(), timeout);
        let stopped = self.process.stop(timeout);
        self.remove();
        stopped
    }

    fn exited(&self) -> Receiver<()> {
        self.process.exited()
    }

    /// False for the same reasons as the native server.
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

#[cfg(test)]
#[path = "container_tests.rs"]
mod tests;
