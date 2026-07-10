use std::ffi::OsStr;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use protocol::{GAME_VERSION, LoginSequence, PROTOCOL_VERSION};

const EXTERNAL_HARNESS_TEST: &str = "^TestProxyExternalRustClientHarness$";
const ENDPOINT_TIMEOUT: Duration = Duration::from_secs(60);
const LOGIN_TIMEOUT: Duration = Duration::from_secs(60);
const CHILD_EXIT_TIMEOUT: Duration = Duration::from_secs(30);
const GO_BUILD_TIMEOUT: Duration = Duration::from_secs(90);

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn login_reaches_start_game_through_bds() {
    if std::env::var_os("BEDROCK_BDS_DIR").is_none() {
        eprintln!("skipping live login: BEDROCK_BDS_DIR is not set");
        return;
    }

    let socket_dir = TestSocketDir::new().expect("create test socket directory");
    let mut harness = GoHarness::spawn(socket_dir.path()).expect("start Go live harness");
    wait_for_endpoint(&mut harness, socket_dir.path())
        .await
        .unwrap_or_else(|error| panic!("{error}\nGo harness output:\n{}", harness.output()));

    let (session, game_data) = tokio::time::timeout(
        LOGIN_TIMEOUT,
        LoginSequence::connect(socket_dir.path(), "RustMCBEPhase0"),
    )
    .await
    .unwrap_or_else(|_| {
        panic!(
            "timed out logging in through the Go core\nGo harness output:\n{}",
            harness.output()
        )
    })
    .unwrap_or_else(|error| {
        panic!(
            "login through the Go core failed: {error}\nGo harness output:\n{}",
            harness.output()
        )
    });

    assert_eq!(PROTOCOL_VERSION, 1001);
    assert_eq!(GAME_VERSION, "1.26.30");
    assert_ne!(
        game_data.start_game.runtime_entity_id, 0,
        "StartGame runtime entity ID must be non-zero"
    );
    assert_eq!(game_data.start_game.engine, GAME_VERSION);

    assert_eq!(
        session.decode_error_count(),
        0,
        "login sequence must not suppress packet decode errors"
    );

    drop(session);
    let finish_result = harness.finish(CHILD_EXIT_TIMEOUT);
    let harness_output = harness.output();
    let status = finish_result.unwrap_or_else(|error| {
        panic!(
            "Go live harness did not stop cleanly: {error}\nGo harness output:\n{}",
            harness_output
        )
    });
    assert!(
        status.success(),
        "Go live harness exited with {status}\nGo harness output:\n{}",
        harness_output
    );
}

async fn wait_for_endpoint(harness: &mut GoHarness, socket_dir: &Path) -> Result<(), String> {
    #[cfg(windows)]
    let endpoint = socket_dir.join("game.addr");
    #[cfg(unix)]
    let endpoint = socket_dir.join("game.sock");

    let deadline = Instant::now() + ENDPOINT_TIMEOUT;
    loop {
        if endpoint.exists() {
            return Ok(());
        }
        if let Some(status) = harness
            .try_wait()
            .map_err(|error| format!("inspect Go harness: {error}"))?
        {
            return Err(format!(
                "Go harness exited with {status} before publishing {}",
                endpoint.display()
            ));
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "timed out waiting for Go harness endpoint {}",
                endpoint.display()
            ));
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

struct GoHarness {
    child: Child,
    stdin: Option<ChildStdin>,
    output: Arc<Mutex<Vec<u8>>>,
    readers: Vec<JoinHandle<()>>,
    completed: bool,
}

impl GoHarness {
    fn spawn(socket_dir: &Path) -> io::Result<Self> {
        let core_dir = project_root().join("core");
        #[cfg(windows)]
        let executable = socket_dir.join("proxy-live-harness.test.exe");
        #[cfg(not(windows))]
        let executable = socket_dir.join("proxy-live-harness.test");
        build_go_harness(&core_dir, &executable)?;

        let mut command = Command::new(&executable);
        command
            .current_dir(core_dir)
            .args([
                OsStr::new("-test.run"),
                OsStr::new(EXTERNAL_HARNESS_TEST),
                OsStr::new("-test.count=1"),
                OsStr::new("-test.v"),
            ])
            .env("RUST_MCBE_EXTERNAL_RUST_CLIENT", "1")
            .env("RUST_MCBE_PROXY_SOCKET_DIR", socket_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = command.spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("Go harness stdin was not piped"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("Go harness stdout was not piped"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("Go harness stderr was not piped"))?;
        let output = Arc::new(Mutex::new(Vec::new()));
        let readers = vec![
            drain_output(stdout, Arc::clone(&output)),
            drain_output(stderr, Arc::clone(&output)),
        ];

        Ok(Self {
            child,
            stdin: Some(stdin),
            output,
            readers,
            completed: false,
        })
    }

    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }

    fn finish(&mut self, timeout: Duration) -> io::Result<ExitStatus> {
        self.stdin.take();
        let status = match wait_for_child(&mut self.child, timeout)? {
            Some(status) => status,
            None => {
                let kill_error = self.child.kill().err();
                let final_status = wait_for_child(&mut self.child, Duration::from_secs(5))
                    .ok()
                    .flatten();
                self.completed = true;
                if final_status.is_some() {
                    self.join_readers();
                } else {
                    self.readers.clear();
                }
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!(
                        "timed out after {timeout:?}; kill error={kill_error:?}; final status={final_status:?}"
                    ),
                ));
            }
        };
        self.completed = true;
        self.join_readers();
        Ok(status)
    }

    fn output(&self) -> String {
        let bytes = self.output.lock().expect("Go harness output lock poisoned");
        String::from_utf8_lossy(&bytes).into_owned()
    }

    fn join_readers(&mut self) {
        for reader in self.readers.drain(..) {
            let _ = reader.join();
        }
    }
}

fn build_go_harness(core_dir: &Path, executable: &Path) -> io::Result<()> {
    let mut command = Command::new("go");
    command
        .current_dir(core_dir)
        .args([OsStr::new("test"), OsStr::new("-c"), OsStr::new("-o")])
        .arg(executable)
        .arg("./proxy")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("Go build stdout was not piped"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("Go build stderr was not piped"))?;
    let output = Arc::new(Mutex::new(Vec::new()));
    let readers = vec![
        drain_output(stdout, Arc::clone(&output)),
        drain_output(stderr, Arc::clone(&output)),
    ];

    let status = match wait_for_child(&mut child, GO_BUILD_TIMEOUT)? {
        Some(status) => status,
        None => {
            let kill_error = child.kill().err();
            let final_status = wait_for_child(&mut child, Duration::from_secs(5))?;
            if final_status.is_some() {
                for reader in readers {
                    let _ = reader.join();
                }
            }
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!(
                    "building Go live harness timed out after {GO_BUILD_TIMEOUT:?}; kill error={kill_error:?}; final status={final_status:?}"
                ),
            ));
        }
    };
    for reader in readers {
        let _ = reader.join();
    }
    if status.success() {
        return Ok(());
    }
    let output = output.lock().expect("Go build output lock poisoned");
    Err(io::Error::other(format!(
        "build Go live harness failed with {status}\noutput:\n{}",
        String::from_utf8_lossy(&output)
    )))
}

impl Drop for GoHarness {
    fn drop(&mut self) {
        if self.completed {
            return;
        }
        self.stdin.take();
        if wait_for_child(&mut self.child, Duration::from_secs(5))
            .ok()
            .flatten()
            .is_none()
        {
            let _ = self.child.kill();
            if wait_for_child(&mut self.child, Duration::from_secs(5))
                .ok()
                .flatten()
                .is_none()
            {
                self.completed = true;
                self.readers.clear();
                return;
            }
        }
        self.completed = true;
        self.join_readers();
    }
}

fn wait_for_child(child: &mut Child, timeout: Duration) -> io::Result<Option<ExitStatus>> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn drain_output<R>(mut reader: R, output: Arc<Mutex<Vec<u8>>>) -> JoinHandle<()>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut chunk = [0u8; 4096];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) | Err(_) => return,
                Ok(read) => output
                    .lock()
                    .expect("Go harness output lock poisoned")
                    .extend_from_slice(&chunk[..read]),
            }
        }
    })
}

fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("protocol crate is nested under crates/")
        .to_path_buf()
}

struct TestSocketDir {
    path: PathBuf,
}

impl TestSocketDir {
    fn new() -> io::Result<Self> {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let base = std::fs::canonicalize(std::env::temp_dir())?;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        for _ in 0..100 {
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let path = base.join(format!(
                "rust-mcbe-login-{}-{timestamp}-{id}",
                std::process::id()
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate a unique socket directory",
        ))
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestSocketDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
