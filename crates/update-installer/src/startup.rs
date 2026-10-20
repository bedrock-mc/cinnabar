//! Native launches and the first-start handshake for a client the helper installed.

use std::{
    fs, io,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

/// Names the `started` marker the launched client creates after its first complete frames.
pub const STARTUP_ENV: &str = "CINNABAR_UPDATE_STARTUP_FILE";
/// The variable an AppImage runtime sets to the image it runs from.
pub const APPIMAGE: &str = "APPIMAGE";
/// The variables an AppImage runtime sets for the program it starts; a launch that does not go
/// through that runtime must not inherit them.
pub const APPIMAGE_RUNTIME_ENV: [&str; 5] =
    [APPIMAGE, "APPDIR", "OWD", "LD_LIBRARY_PATH", "LD_PRELOAD"];
/// The marker's filename; the client accepts no other path from the environment.
pub const STARTED: &str = "started";
/// Longest silence, before startup or since the last preparation heartbeat, that counts as stalled;
/// time spent awaiting the user never counts.
pub const STALL_TIMEOUT: Duration = Duration::from_secs(300);
/// How often a preparing client looks for new progress; well under the stall timeout.
pub const PROGRESS_POLL: Duration = Duration::from_secs(10);
const PREPARING: &str = "preparing";
/// Heartbeat content while setup waits on the user; counted heartbeats are digits, never this.
const AWAITING_USER: &str = "awaiting-user";
/// How long a stopped client may exit cleanly before the helper kills what remains.
#[cfg(unix)]
const STOP_GRACE: Duration = Duration::from_secs(10);

/// Builds a native client launch without inheriting another helper's startup receipt.
pub fn client_command(target: &Path) -> Command {
    #[cfg(target_os = "macos")]
    let executable = target.join("Contents/MacOS/cinnabar");
    #[cfg(windows)]
    let executable = target.join(update_manifest::runtime::executable("CLIENT", "windows"));
    #[cfg(not(any(target_os = "macos", windows)))]
    let executable = target.to_owned();
    let mut command = Command::new(executable);
    command
        .env_remove(STARTUP_ENV)
        .stdin(Stdio::null())
        .stdout(Stdio::null());
    #[cfg(target_os = "linux")]
    {
        command.arg("--appimage-extract-and-run");
        for name in APPIMAGE_RUNTIME_ENV {
            command.env_remove(name);
        }
    }
    command
}

/// The marker path handed to the client launched from `directory`.
pub fn marker(directory: &Path) -> PathBuf {
    directory.join(STARTED)
}

/// Records heartbeat `content` beside `marker`; each new content restarts the helper's deadline.
fn beat(marker: &Path, content: &str) -> io::Result<()> {
    let directory = marker
        .parent()
        .ok_or_else(|| io::Error::other("startup marker has no directory"))?;
    fs::write(directory.join(PREPARING), content)
}

/// Client side: one heartbeat per change to the setup's progress report, so a hung setup stalls,
/// and an awaiting-user state while the report says setup waits on the user.
pub struct Relay {
    marker: PathBuf,
    progress: PathBuf,
    awaiting_user: fn(&[u8]) -> bool,
    last: Option<Vec<u8>>,
    waiting: bool,
    count: u64,
}

impl Relay {
    /// Starts relaying `progress`; entering preparation is itself progress.
    pub fn start(marker: PathBuf, progress: PathBuf, awaiting_user: fn(&[u8]) -> bool) -> Self {
        let last = fs::read(&progress).ok();
        let mut relay = Self {
            marker,
            progress,
            awaiting_user,
            last,
            waiting: false,
            count: 0,
        };
        relay.beat();
        relay
    }

    /// Beats when the progress report changed or stopped awaiting the user.
    pub fn poll(&mut self) {
        let report = fs::read(&self.progress).ok();
        let waiting = report.as_deref().is_some_and(self.awaiting_user);
        let changed = report.is_some() && report != self.last;
        if report.is_some() {
            self.last = report;
        }
        if waiting {
            if !self.waiting {
                self.waiting = true;
                let _ = beat(&self.marker, AWAITING_USER);
            }
        } else if changed || self.waiting {
            self.beat();
        }
    }

    /// Ends preparation, giving the rest of startup a fresh deadline.
    pub fn finish(mut self) {
        self.beat();
    }

    fn beat(&mut self) {
        self.waiting = false;
        self.count += 1;
        let _ = beat(&self.marker, &self.count.to_string());
    }
}

/// How far the launched client's first start has progressed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Progress {
    Started,
    Waiting,
    Stalled,
}

/// Helper-side deadline that every distinct preparation heartbeat restarts.
pub struct Monitor {
    deadline: Instant,
    beat: Option<String>,
}

impl Monitor {
    pub fn new(now: Instant) -> Self {
        Self {
            deadline: now + STALL_TIMEOUT,
            beat: None,
        }
    }

    /// Reads the handshake in `directory`; `now` comes from the caller.
    pub fn poll(&mut self, directory: &Path, now: Instant) -> Progress {
        if directory.join(STARTED).is_file() {
            return Progress::Started;
        }
        let beat = fs::read_to_string(directory.join(PREPARING)).ok();
        if beat.is_some() && beat != self.beat {
            self.beat = beat;
            self.deadline = now + STALL_TIMEOUT;
        }
        // Leaving this state is a new heartbeat, so the deadline restarts when the user answers.
        if self.beat.as_deref() == Some(AWAITING_USER) {
            return Progress::Waiting;
        }
        if now >= self.deadline {
            Progress::Stalled
        } else {
            Progress::Waiting
        }
    }
}

/// The launched client and every process it starts: a process group on Unix, a job on Windows.
pub struct Tree {
    child: Child,
    /// Absent only if Windows refused the job, leaving `stop` to end the client alone.
    #[cfg(windows)]
    job: Option<job::Job>,
}

impl Tree {
    /// Spawns `command` as the root of a new tree.
    pub fn spawn(command: &mut Command) -> io::Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let child = command.spawn()?;
        // The client starts setup only after it initializes, well after this assignment.
        #[cfg(windows)]
        let job = job::Job::assign(&child).ok();
        Ok(Self {
            child,
            #[cfg(windows)]
            job,
        })
    }

    /// The client's exit status, once it has exited.
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }

    /// Returns once the client and every process it started have exited.
    pub fn stop(mut self) -> io::Result<()> {
        #[cfg(unix)]
        {
            use rustix::process::{Pid, Signal, kill_process_group, test_kill_process_group};
            let group = Pid::from_child(&self.child);
            for signal in [Signal::TERM, Signal::KILL] {
                let _ = kill_process_group(group, signal);
                let deadline = Instant::now() + STOP_GRACE;
                // Reaping the leader keeps its zombie from holding the group open.
                while self.child.try_wait()?.is_none() || test_kill_process_group(group).is_ok() {
                    if signal == Signal::TERM && Instant::now() >= deadline {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }
        #[cfg(windows)]
        match &self.job {
            Some(job) => job.terminate()?,
            None => {
                let _ = self.child.kill();
            }
        }
        #[cfg(not(any(unix, windows)))]
        let _ = self.child.kill();
        self.child.wait().map(drop)
    }
}

#[cfg(windows)]
mod job {
    use std::{io, os::windows::io::AsRawHandle, process::Child, time::Duration};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
            JobObjectBasicAccountingInformation, QueryInformationJobObject, TerminateJobObject,
        },
    };

    /// Holds the client's process tree; closing it leaves a healthy client running.
    pub(super) struct Job(HANDLE);

    impl Job {
        pub(super) fn assign(child: &Child) -> io::Result<Self> {
            // SAFETY: the new handle is checked and owned by `Job`; the child handle stays valid.
            unsafe {
                let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if handle.is_null() {
                    return Err(io::Error::last_os_error());
                }
                let job = Self(handle);
                if AssignProcessToJobObject(job.0, child.as_raw_handle()) == 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(job)
            }
        }

        /// Terminates every process in the job and returns once none remain.
        pub(super) fn terminate(&self) -> io::Result<()> {
            // SAFETY: the job handle is valid, and the query writes only into `info`.
            unsafe {
                if TerminateJobObject(self.0, 1) == 0 {
                    return Err(io::Error::last_os_error());
                }
                loop {
                    let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
                    let queried = QueryInformationJobObject(
                        self.0,
                        JobObjectBasicAccountingInformation,
                        (&raw mut info).cast(),
                        size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                        std::ptr::null_mut(),
                    );
                    if queried == 0 {
                        return Err(io::Error::last_os_error());
                    }
                    if info.ActiveProcesses == 0 {
                        return Ok(());
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            // SAFETY: `Job` owns this handle and closes it exactly once.
            unsafe { CloseHandle(self.0) };
        }
    }
}

#[cfg(test)]
mod tests;
