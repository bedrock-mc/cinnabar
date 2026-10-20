//! Fixtures shared by the local-world tests: worlds, archives, a fake download service, fake
//! servers and an in-memory runner.

use std::{
    io::{BufRead, BufReader, Cursor, Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

use bridge::{Backend, GameMode};
use crossbeam_channel::{Receiver, Sender};
use launcher::local_worlds::world::{Difficulty, Generator, World};
use url::Url;

use super::{
    Cancel, lock,
    process::{Instance, Runner, StartSpec},
    provision::{Platform, Provisioner},
    world::WorldError,
};

pub(crate) const VERSION: &str = "1.26.52.3";
pub(crate) const AGENT: &str = "Cinnabar-local-worlds";

/// A fresh temporary folder, removed when dropped.
pub(crate) fn scratch() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("cinnabar-local-worlds-")
        .tempdir()
        .expect("temporary folder")
}

pub(crate) fn world() -> World {
    World {
        id: "0123456789abcdef".to_owned(),
        name: "n".to_owned(),
        game_mode: GameMode::Survival,
        generator: Generator::Flat,
        difficulty: Difficulty::Easy,
        backend: Backend::Bds,
        seed: -7,
        created_unix: 0,
        last_played_unix: 0,
        size_bytes: 0,
    }
}

pub(crate) fn spec(dir: &Path) -> StartSpec {
    StartSpec {
        world: world(),
        dir: dir.to_path_buf(),
        view_distance: None,
    }
}

/// A deflated archive of `files`.
pub(crate) fn build_zip(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, body) in files {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .expect("zip entry");
        zip.write_all(body).expect("zip body");
    }
    zip.finish().expect("zip").into_inner()
}

/// An archive large enough that half of it is a meaningful resume point.
pub(crate) fn incompressible_zip() -> Vec<u8> {
    let mut binary = vec![0u8; 256 << 10];
    getrandom::fill(&mut binary).expect("random bytes");
    build_zip(&[("bedrock_server", &binary), ("server.properties", b"x")])
}

/// A loopback stand-in for Mojang's download service: `/links` lists `bin-linux/` and `bin-win/`
/// archives of one version, and archive requests honour Range and the downloader's agent.
pub(crate) struct FakeMojang {
    pub base: String,
    pub hits: Arc<AtomicUsize>,
    /// The Range header of each archive request; empty when none was sent.
    pub ranges: Arc<Mutex<Vec<String>>>,
    /// When positive, archive replies send that many bytes and then go silent.
    pub stall_after: Arc<AtomicUsize>,
}

impl FakeMojang {
    pub(crate) fn start(version: &str, archive: Vec<u8>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake download service");
        let base = format!("http://{}", listener.local_addr().expect("address"));
        let fake = Self {
            base: base.clone(),
            hits: Arc::default(),
            ranges: Arc::default(),
            stall_after: Arc::default(),
        };
        let (hits, ranges, stall_after) = (
            Arc::clone(&fake.hits),
            Arc::clone(&fake.ranges),
            Arc::clone(&fake.stall_after),
        );
        let (version, archive) = (version.to_owned(), Arc::new(archive));
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let request = Request {
                    base: base.clone(),
                    version: version.clone(),
                    archive: Arc::clone(&archive),
                    hits: Arc::clone(&hits),
                    ranges: Arc::clone(&ranges),
                    stall_after: Arc::clone(&stall_after),
                };
                thread::spawn(move || request.serve(stream));
            }
        });
        fake
    }

    pub(crate) fn port(&self) -> u16 {
        Url::parse(&self.base)
            .ok()
            .and_then(|url| url.port())
            .expect("fake service port")
    }

    /// The last archive Range header, `None` before any archive request.
    pub(crate) fn last_range(&self) -> Option<String> {
        lock(&self.ranges).last().cloned()
    }

    /// A Linux provisioner pinned to the `1.26.52` line that downloads only from this service.
    pub(crate) fn provisioner(&self, root: &Path) -> Provisioner {
        let mut provisioner = Provisioner::new(root.join("bds"), String::new());
        provisioner.platform = Platform {
            os: "linux",
            arch: "amd64",
        };
        provisioner.version_prefix = "1.26.52".to_owned();
        provisioner.links_url = format!("{}/links", self.base);
        let port = self.port();
        provisioner.allow_host = Arc::new(move |url: &Url| {
            url.host_str() == Some("127.0.0.1") && url.port() == Some(port)
        });
        provisioner
    }
}

struct Request {
    base: String,
    version: String,
    archive: Arc<Vec<u8>>,
    hits: Arc<AtomicUsize>,
    ranges: Arc<Mutex<Vec<String>>>,
    stall_after: Arc<AtomicUsize>,
}

impl Request {
    fn serve(self, mut stream: TcpStream) {
        let Ok(reader) = stream.try_clone() else {
            return;
        };
        let mut reader = BufReader::new(reader);
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() {
            return;
        }
        let path = line
            .split_whitespace()
            .nth(1)
            .unwrap_or_default()
            .to_owned();
        let (mut agent, mut range) = (String::new(), String::new());
        loop {
            let mut header = String::new();
            if reader.read_line(&mut header).unwrap_or(0) == 0 || header.trim().is_empty() {
                break;
            }
            if let Some((name, value)) = header.split_once(':') {
                match name.trim().to_ascii_lowercase().as_str() {
                    "user-agent" => agent = value.trim().to_owned(),
                    "range" => range = value.trim().to_owned(),
                    _ => {}
                }
            }
        }
        self.hits.fetch_add(1, Ordering::SeqCst);
        if path == "/links" {
            let body = format!(
                r#"{{"result":{{"links":[{{"downloadType":"serverBedrockLinux","downloadUrl":"{base}/bin-linux/bedrock-server-{version}.zip"}},{{"downloadType":"serverBedrockWindows","downloadUrl":"{base}/bin-win/bedrock-server-{version}.zip"}}]}}}}"#,
                base = self.base,
                version = self.version
            );
            respond(&mut stream, "200 OK", "", body.as_bytes());
            return;
        }
        if !path.starts_with("/bin-linux/") {
            respond(&mut stream, "404 Not Found", "", b"");
            return;
        }
        // minecraft.net resets requests that do not name their downloader.
        if agent != AGENT {
            respond(&mut stream, "403 Forbidden", "", b"agent");
            return;
        }
        lock(&self.ranges).push(range.clone());
        let archive = self.archive.as_slice();
        let stall = self.stall_after.load(Ordering::SeqCst);
        if stall > 0 {
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                archive.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&archive[..stall]);
            let _ = stream.flush();
            // Silent until the client gives up and closes the connection.
            let _ = reader.read_to_end(&mut Vec::new());
            return;
        }
        let from = range
            .strip_prefix("bytes=")
            .and_then(|range| range.strip_suffix('-'))
            .and_then(|from| from.parse::<usize>().ok());
        match from {
            Some(from) if from < archive.len() => {
                let header = format!(
                    "Content-Range: bytes {from}-{}/{}\r\n",
                    archive.len() - 1,
                    archive.len()
                );
                respond(
                    &mut stream,
                    "206 Partial Content",
                    &header,
                    &archive[from..],
                );
            }
            Some(_) => {
                let header = format!("Content-Range: bytes */{}\r\n", archive.len());
                respond(&mut stream, "416 Range Not Satisfiable", &header, b"");
            }
            None => respond(&mut stream, "200 OK", "", archive),
        }
    }
}

fn respond(stream: &mut TcpStream, status: &str, headers: &str, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

/// A provisioner with a fake build of [`VERSION`] already installed and the EULA accepted.
#[cfg(unix)]
pub(crate) fn installed_provisioner(root: &Path, platform: Platform, binary: &[u8]) -> Provisioner {
    let mut provisioner = Provisioner::new(root.join("bds"), VERSION.to_owned());
    provisioner.platform = platform;
    let dir = provisioner.root.join(VERSION);
    std::fs::create_dir_all(&dir).expect("install folder");
    std::fs::write(dir.join("manifest.json"), b"{}").expect("manifest");
    write_script(&dir.join("bedrock_server"), binary);
    provisioner.accept_eula().expect("accept EULA");
    provisioner
}

/// Writes an executable file.
#[cfg(unix)]
pub(crate) fn write_script(path: &Path, body: &[u8]) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, body).expect("write script");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .expect("make script executable");
}

/// A server that announces BDS readiness and stops on a `stop` line or stdin EOF.
#[cfg(unix)]
pub(crate) const FAKE_BDS: &[u8] = b"#!/bin/sh\nprintf '[INFO] Starting Server\\r\\n[INFO] Server started.\\r\\n'\nwhile IFS= read -r line; do\n  case \"$line\" in stop*) exit 0 ;; esac\ndone\n";

/// A Docker CLI that logs each invocation to `$FAKE_DOCKER_LOG` and answers the subcommands
/// the container runner uses; `$FAKE_DOCKER_DOWN` makes `info` fail and `$FAKE_DOCKER_HANG` makes
/// `image` hang.
#[cfg(unix)]
pub(crate) const FAKE_DOCKER: &[u8] = b"#!/bin/sh\nlog=\"$FAKE_DOCKER_LOG\"\necho \"$*\" >> \"$log\"\ncase \"$1\" in\n  info) [ -z \"$FAKE_DOCKER_DOWN\" ] ;;\n  image) while [ -n \"$FAKE_DOCKER_HANG\" ]; do sleep 0.05; done; [ -e \"$log.pulled\" ] ;;\n  pull) : > \"$log.pulled\" ;;\n  stop) : > \"$log.stop\" ;;\n  run) echo '[INFO] Server started.'; while [ ! -e \"$log.stop\" ]; do sleep 0.05; done ;;\nesac\n";

/// Records every pause request; exits once stopped.
pub(crate) struct FakeInstance {
    pub paused: Mutex<Vec<bool>>,
    exit: Mutex<Option<Sender<()>>>,
    exited: Receiver<()>,
    pub can_pause: bool,
    pub lan: Option<String>,
    /// Holds a stop, as a world save would, until disconnected.
    save: Option<Receiver<()>>,
    /// Holds a stop after the process has exited, as removing a container would.
    cleanup: Option<Receiver<()>>,
    cleaned: std::sync::atomic::AtomicBool,
}

impl FakeInstance {
    pub(crate) fn new(can_pause: bool) -> Self {
        let (exit, exited) = crossbeam_channel::bounded(0);
        Self {
            paused: Mutex::default(),
            exit: Mutex::new(Some(exit)),
            exited,
            can_pause,
            lan: None,
            save: None,
            cleanup: None,
            cleaned: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Ends the fake server as if it crashed.
    pub(crate) fn exit(&self) {
        lock(&self.exit).take();
    }

    pub(crate) fn stopped(&self) -> bool {
        lock(&self.exit).is_none()
    }

    /// Stopped, with any cleanup after the exit done.
    pub(crate) fn finished(&self) -> bool {
        self.stopped()
            && (self.cleanup.is_none() || self.cleaned.load(std::sync::atomic::Ordering::SeqCst))
    }
}

impl Instance for FakeInstance {
    fn address(&self) -> &str {
        "127.0.0.1:1"
    }

    fn set_paused(&self, paused: bool) -> std::io::Result<()> {
        lock(&self.paused).push(paused);
        Ok(())
    }

    fn request_stop(&self) {
        if self.save.is_none() {
            self.exit();
        }
    }

    fn stop(&self, _timeout: Duration) -> Result<(), String> {
        if let Some(save) = &self.save {
            let _ = save.recv();
        }
        self.exit();
        if let Some(cleanup) = &self.cleanup {
            let _ = cleanup.recv();
            self.cleaned
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(())
    }

    fn exited(&self) -> Receiver<()> {
        self.exited.clone()
    }

    fn can_pause(&self) -> bool {
        self.can_pause
    }

    fn lan_address(&self) -> Option<&str> {
        self.lan.as_deref()
    }
}

/// Starts [`FakeInstance`]s, optionally held until `release` or failing with `error`.
#[derive(Default)]
pub(crate) struct FakeRunner {
    pub gate: Option<Receiver<()>>,
    pub error: Option<fn() -> WorldError>,
    pub can_pause: Option<bool>,
    pub made: Mutex<Vec<Arc<FakeInstance>>>,
    /// Passed to each instance as its save.
    pub save: Option<Receiver<()>>,
    /// Passed to each instance as its cleanup.
    pub cleanup: Option<Receiver<()>>,
    pub discarded: std::sync::atomic::AtomicUsize,
    /// Set when a start ran while an earlier instance was still alive.
    pub overlapped: std::sync::atomic::AtomicBool,
    pub prepared: std::sync::atomic::AtomicUsize,
}

impl FakeRunner {
    /// A runner whose starts wait until the returned sender is dropped.
    pub(crate) fn gated() -> (Self, Sender<()>) {
        let (release, gate) = crossbeam_channel::bounded(0);
        (
            Self {
                gate: Some(gate),
                ..Self::default()
            },
            release,
        )
    }

    /// A runner whose instances save until the returned sender is dropped.
    pub(crate) fn saving() -> (Self, Sender<()>) {
        let (release, save) = crossbeam_channel::bounded(0);
        (
            Self {
                save: Some(save),
                ..Self::default()
            },
            release,
        )
    }

    /// A runner whose instances exit at once but finish cleaning up when the sender is dropped.
    pub(crate) fn cleaning() -> (Self, Sender<()>) {
        let (release, cleanup) = crossbeam_channel::bounded(0);
        (
            Self {
                cleanup: Some(cleanup),
                ..Self::default()
            },
            release,
        )
    }

    pub(crate) fn last(&self) -> Arc<FakeInstance> {
        Arc::clone(lock(&self.made).last().expect("a started instance"))
    }
}

impl Runner for FakeRunner {
    fn start(&self, _spec: &StartSpec, cancel: &Cancel) -> Result<Arc<dyn Instance>, WorldError> {
        if let Some(gate) = &self.gate {
            crossbeam_channel::select! {
                recv(gate) -> _ => {}
                recv(cancel.receiver()) -> _ => return Err(WorldError::Cancelled),
            }
        }
        if let Some(error) = self.error {
            return Err(error());
        }
        let mut made = lock(&self.made);
        if made.iter().any(|instance| !instance.finished()) {
            self.overlapped
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }
        let instance = Arc::new(FakeInstance {
            save: self.save.clone(),
            cleanup: self.cleanup.clone(),
            ..FakeInstance::new(self.can_pause.unwrap_or(true))
        });
        made.push(Arc::clone(&instance));
        drop(made);
        let instance: Arc<dyn Instance> = instance;
        Ok(instance)
    }

    fn prepare(&self) {
        self.prepared
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    fn discard_standby(&self) {
        self.discarded
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}
