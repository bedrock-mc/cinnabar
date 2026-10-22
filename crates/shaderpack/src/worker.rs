//! CPU translation in a disposable process with memory, CPU and elapsed-time limits.

use crate::{Environment, OptionValues, PackTranslation, ShaderPack, TranslateConfig};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{self, Read, Seek, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::Duration,
};

mod allocator;
pub use allocator::BudgetAllocator;

/// Internal compiler entry point; it starts no window, renderer or account services.
pub const FLAG: &str = "--shaderpack-compiler-worker";
/// Maximum elapsed time for CPU translation, supplied to [`Running::poll`] by the host.
pub const TIME_LIMIT: Duration = Duration::from_secs(60);
/// Maximum address space on Linux and committed process memory on Windows.
const MEMORY_LIMIT: u64 = 2 * 1024 * 1024 * 1024;
/// Maximum serialized request and output sizes, checked before deserialization.
const REQUEST_BYTES: usize = 1024 * 1024;
const OUTPUT_BYTES: usize = 128 * 1024 * 1024;

/// Inputs read only after the child has installed its resource limits.
#[derive(Serialize, Deserialize)]
pub struct Request {
    pub pack: PathBuf,
    pub options: Option<PathBuf>,
    pub dimension: String,
    pub environment: Environment,
    pub purpose: Purpose,
}

/// The untrusted CPU work requested by the client.
#[derive(Serialize, Deserialize)]
pub enum Purpose {
    Render,
    Options,
}

/// A typed result prevents options readers from accepting a render translation or vice versa.
#[derive(Serialize, Deserialize)]
pub enum Response {
    Render(Box<Output>),
    Options(Box<crate::OptionSession>),
}

impl Response {
    /// Accepts only the completed render translation the loader requested.
    pub fn into_translation(self) -> Result<Output, String> {
        match self {
            Self::Render(output) => Ok(*output),
            Self::Options(_) => Err("unexpected shader options result".into()),
        }
    }

    /// Accepts only the option session the editor requested.
    pub fn into_options(self) -> Result<crate::OptionSession, String> {
        match self {
            Self::Options(session) => Ok(*session),
            Self::Render(_) => Err("unexpected shader translation result".into()),
        }
    }
}

/// Owned data handed to the host after a successful bounded translation.
#[derive(Serialize, Deserialize)]
pub struct Output {
    pub pack: ShaderPack,
    pub config: TranslateConfig,
    pub translation: PackTranslation,
    pub custom_uniforms: (crate::CustomUniforms, Vec<crate::CustomUniformError>),
    pub ids: crate::IdMaps,
    pub block_ids: Option<Vec<i32>>,
}

/// One child owned until it exits or is killed and reaped; dropping it cannot detach work.
pub struct Running {
    child: Child,
    output: File,
    #[cfg(windows)]
    _job: windows::Job,
}

impl Running {
    /// Starts `executable` in compiler mode and transfers the bounded request over stdin.
    pub fn start(executable: &Path, request: &Request) -> Result<Self, String> {
        let input = serde_json::to_vec(request).map_err(|error| error.to_string())?;
        if input.len() > REQUEST_BYTES {
            return Err("shader compiler request exceeds storage limit".into());
        }
        let output = tempfile::tempfile().map_err(|error| error.to_string())?;
        let mut command = Command::new(executable);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
        }
        let child = command
            .arg(FLAG)
            .stdin(Stdio::piped())
            .stdout(output.try_clone().map_err(|error| error.to_string())?)
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("cannot start shader compiler: {error}"))?;
        #[cfg(windows)]
        let job = match windows::Job::attach(&child) {
            Ok(job) => job,
            Err(error) => {
                let mut child = child;
                let _ = child.kill();
                let _ = child.wait();
                return Err(error.to_string());
            }
        };
        let mut running = Self {
            child,
            output,
            #[cfg(windows)]
            _job: job,
        };
        let mut stdin = running
            .child
            .stdin
            .take()
            .ok_or("shader compiler stdin missing")?;
        stdin.write_all(&input).map_err(|error| error.to_string())?;
        Ok(running)
    }

    /// Returns a completed translation, or kills and reaps this child on cancellation or expiry.
    /// The caller supplies elapsed time; polling itself never sleeps or reads a clock.
    pub fn poll(&mut self, elapsed: Duration, cancelled: bool) -> Option<Result<Response, String>> {
        if cancelled || elapsed >= TIME_LIMIT {
            let _ = self.child.kill();
            let _ = self.child.wait();
            return Some(Err(if cancelled {
                "shader pack load cancelled".into()
            } else {
                "shader compiler exceeded its time limit".into()
            }));
        }
        let status = match self.child.try_wait() {
            Ok(Some(status)) => status,
            Ok(None) => return None,
            Err(error) => return Some(Err(error.to_string())),
        };
        Some((|| {
            if !status.success() {
                return Err(
                    "shader compiler stopped before completing (invalid input or resource limit)"
                        .into(),
                );
            }
            self.output.rewind().map_err(|error| error.to_string())?;
            let bytes =
                read_limited(&mut self.output, OUTPUT_BYTES).map_err(|error| error.to_string())?;
            let result: Result<Response, String> = ciborium::from_reader(bytes.as_slice())
                .map_err(|error| format!("invalid shader compiler result: {error}"))?;
            result
        })())
    }
}

impl Drop for Running {
    /// Stops and reaps only the compiler process this handle started.
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Runs compiler mode before normal host startup; every failure exits unsuccessfully.
/// Executables that call this must install [`BudgetAllocator`] as their global allocator.
pub fn run_stdio() -> i32 {
    let result = (|| -> Result<(), String> {
        if !allocator::enable() {
            return Err("the compiler host has no heap budget allocator".into());
        }
        resource_limits().map_err(|error| error.to_string())?;
        // The child also expires if its supervising client exits while a file read is blocked.
        std::thread::Builder::new()
            .name("shader compiler deadline".into())
            .spawn(|| {
                let started = std::time::Instant::now();
                while started.elapsed() < TIME_LIMIT {
                    std::thread::park_timeout(TIME_LIMIT.saturating_sub(started.elapsed()));
                }
                // Abort skips Rust's stdio cleanup, which may be locked by a stalled writer.
                std::process::abort();
            })
            .map_err(|error| error.to_string())?;
        let input =
            read_limited(io::stdin().lock(), REQUEST_BYTES).map_err(|error| error.to_string())?;
        let request: Request = serde_json::from_slice(&input).map_err(|error| error.to_string())?;
        let result = translate(request);
        ciborium::into_writer(
            &result,
            LimitedWriter {
                writer: io::stdout().lock(),
                remaining: OUTPUT_BYTES,
            },
        )
        .map_err(|error| error.to_string())
    })();
    i32::from(result.is_err())
}

/// Opens, normalizes and translates one pack entirely within the child's limits.
fn translate(request: Request) -> Result<Response, String> {
    let pack = ShaderPack::open(&request.pack).map_err(|error| error.to_string())?;
    let saved = request
        .options
        .as_deref()
        .map(crate::read_option_file)
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or_else(OptionValues::new);
    if matches!(request.purpose, Purpose::Options) {
        return Ok(Response::Options(Box::new(crate::OptionSession::new(
            &pack, &saved,
        ))));
    }
    let config = TranslateConfig {
        folder: pack.dimension_folder(&request.dimension, &request.environment),
        options: crate::normalize_values(pack.options(), &saved),
        environment: request.environment,
        ..Default::default()
    };
    let translation = pack.translate(&config);
    Ok(Response::Render(Box::new(prepare_translation(
        pack,
        config,
        translation,
    ))))
}

/// Finishes CPU preparation inside a bounded compiler or a trusted offline fixture.
pub fn prepare_translation(
    pack: ShaderPack,
    config: TranslateConfig,
    translation: PackTranslation,
) -> Output {
    let custom_uniforms = crate::CustomUniforms::new(&translation.properties.custom_uniforms);
    let ids = pack.id_maps(&config.environment, &config.options);
    let block_ids = assets::facts::JavaBlockMapping::pinned()
        .ok()
        .map(|mapping| ids.blocks.bedrock_block_ids(mapping));
    Output {
        pack,
        config,
        translation,
        custom_uniforms,
        ids,
        block_ids,
    }
}

/// Reads at most one byte beyond a payload limit without trusting a declared length.
fn read_limited(reader: impl Read, limit: usize) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::FileTooLarge,
            "shader compiler payload exceeds storage limit",
        ));
    }
    Ok(bytes)
}

/// Prevents a successful compiler from retaining or writing an unbounded result.
struct LimitedWriter<W> {
    writer: W,
    remaining: usize,
}

impl<W: Write> Write for LimitedWriter<W> {
    /// Writes only within the result budget and reports exhaustion as an ordinary I/O error.
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.remaining {
            return Err(io::Error::new(
                io::ErrorKind::FileTooLarge,
                "shader compiler output exceeds storage limit",
            ));
        }
        let written = self.writer.write(bytes)?;
        self.remaining -= written;
        Ok(written)
    }

    /// Flushes the underlying result stream.
    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

/// Installs OS limits before any pack bytes are read; unavailable isolation fails closed.
#[cfg(unix)]
fn resource_limits() -> io::Result<()> {
    #[cfg(not(target_os = "macos"))]
    let memory = libc::rlimit {
        rlim_cur: MEMORY_LIMIT as _,
        rlim_max: MEMORY_LIMIT as _,
    };
    let cpu = libc::rlimit {
        rlim_cur: TIME_LIMIT.as_secs() as _,
        rlim_max: TIME_LIMIT.as_secs() as _,
    };
    let output = libc::rlimit {
        rlim_cur: OUTPUT_BYTES as _,
        rlim_max: OUTPUT_BYTES as _,
    };
    let core = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: setrlimit reads valid local structures and changes only this disposable child.
    unsafe {
        #[cfg(not(target_os = "macos"))]
        if libc::setrlimit(libc::RLIMIT_AS, &memory) != 0 {
            return Err(io::Error::last_os_error());
        }
        if libc::setrlimit(libc::RLIMIT_CPU, &cpu) != 0
            || libc::setrlimit(libc::RLIMIT_FSIZE, &output) != 0
            || libc::setrlimit(libc::RLIMIT_CORE, &core) != 0
        {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

/// Windows limits belong to the parent-held job and are set before stdin is transferred.
#[cfg(windows)]
fn resource_limits() -> io::Result<()> {
    Ok(())
}

/// Unsupported process hosts reject compiler mode instead of running without limits.
#[cfg(not(any(unix, windows)))]
fn resource_limits() -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "isolated shader compilation is unavailable",
    ))
}

#[cfg(windows)]
mod windows;

#[cfg(test)]
mod tests;
