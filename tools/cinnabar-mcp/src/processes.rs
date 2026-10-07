//! Child processes the server owns: the client and an optional local Dragonfly server.

use std::{
    fs::File,
    io::{BufRead, BufReader, Write},
    net::{Ipv4Addr, UdpSocket},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use developer_control::endpoint::Endpoint;

const POLL: Duration = Duration::from_millis(100);
const EXIT_GRACE: Duration = Duration::from_secs(10);
const SERVER_READY_TIMEOUT: Duration = Duration::from_secs(60);
/// The local server's stdout line once it is listening.
const SERVER_READY_LINE: &str = "ready";

pub struct Client {
    child: Child,
    pub log: PathBuf,
}

pub struct Launch<'a> {
    pub binary: &'a Path,
    pub args: &'a [String],
    pub env: &'a [(String, String)],
    pub endpoint: &'a Path,
    pub log: PathBuf,
    pub timeout: Duration,
}

impl Client {
    /// Starts the client and waits until it publishes `launch.endpoint`.
    pub fn launch(launch: Launch<'_>) -> Result<(Self, Endpoint), String> {
        if !launch.binary.is_file() {
            return Err(format!(
                "{} does not exist; build it with `cargo build -p bedrock-client --features developer-control`",
                launch.binary.display()
            ));
        }
        let _ = std::fs::remove_file(launch.endpoint);
        if let Some(parent) = launch.log.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let log = File::create(&launch.log).map_err(|error| error.to_string())?;
        let child = Command::new(launch.binary)
            .args(launch.args)
            .envs(launch.env.iter().map(|(key, value)| (key, value)))
            .stdin(Stdio::null())
            .stdout(log.try_clone().map_err(|error| error.to_string())?)
            .stderr(log)
            .spawn()
            .map_err(|error| format!("start {}: {error}", launch.binary.display()))?;
        let mut client = Self {
            child,
            log: launch.log,
        };
        let deadline = Instant::now() + launch.timeout;
        loop {
            if let Ok(Some(status)) = client.child.try_wait() {
                return Err(format!(
                    "the client exited ({status}) before its control endpoint came up; see {}",
                    client.log.display()
                ));
            }
            if let Ok(endpoint) = Endpoint::read(launch.endpoint)
                && endpoint.pid == client.child.id()
            {
                return Ok((client, endpoint));
            }
            if Instant::now() >= deadline {
                client.stop();
                return Err(format!(
                    "no control endpoint within {} s; is the binary built with --features developer-control? See {}",
                    launch.timeout.as_secs(),
                    client.log.display()
                ));
            }
            thread::sleep(POLL);
        }
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    pub fn running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Waits for a requested exit, then kills.
    pub fn stop(&mut self) {
        wait_or_kill(&mut self.child);
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub struct LocalServer {
    child: Child,
    /// Held open: the server stops on stdin EOF.
    stdin: Option<ChildStdin>,
    pub address: String,
    pub log: PathBuf,
}

impl LocalServer {
    pub fn start(
        binary: &Path,
        world: &Path,
        extra: &[String],
        log: PathBuf,
    ) -> Result<Self, String> {
        if !binary.is_file() {
            return Err(format!(
                "{} does not exist; build it with `make local-server`",
                binary.display()
            ));
        }
        std::fs::create_dir_all(world).map_err(|error| error.to_string())?;
        let address = format!("127.0.0.1:{}", free_udp_port()?);
        let mut log_file = File::create(&log).map_err(|error| error.to_string())?;
        let mut child = Command::new(binary)
            .arg("-dir")
            .arg(world)
            .arg("-addr")
            .arg(&address)
            .args(extra)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(log_file.try_clone().map_err(|error| error.to_string())?)
            .spawn()
            .map_err(|error| format!("start {}: {error}", binary.display()))?;
        let stdin = child.stdin.take();
        let stdout = child
            .stdout
            .take()
            .ok_or("local server stdout unavailable")?;
        let (ready, readiness) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let _ = writeln!(log_file, "{line}");
                if line.trim() == SERVER_READY_LINE {
                    let _ = ready.send(());
                }
            }
        });
        let mut server = Self {
            child,
            stdin,
            address,
            log,
        };
        if readiness.recv_timeout(SERVER_READY_TIMEOUT).is_err() {
            server.stop();
            return Err(format!(
                "the local server never reported ready; see {}",
                server.log.display()
            ));
        }
        Ok(server)
    }

    pub fn stop(&mut self) {
        if let Some(mut stdin) = self.stdin.take() {
            let _ = writeln!(stdin, "stop");
        }
        wait_or_kill(&mut self.child);
    }
}

impl Drop for LocalServer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn wait_or_kill(child: &mut Child) {
    let deadline = Instant::now() + EXIT_GRACE;
    while Instant::now() < deadline {
        if !matches!(child.try_wait(), Ok(None)) {
            return;
        }
        thread::sleep(POLL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// RakNet listens on UDP; borrow a port the OS just proved free.
fn free_udp_port() -> Result<u16, String> {
    UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
        .and_then(|socket| socket.local_addr())
        .map(|address| address.port())
        .map_err(|error| format!("no free loopback UDP port: {error}"))
}
