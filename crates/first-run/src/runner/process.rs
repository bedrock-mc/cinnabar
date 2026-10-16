//! Owns the compiler until reaped and drains its pipes after stopping it on failure.

#[cfg(windows)]
use std::process::Stdio;
use std::{
    io::{self, BufRead, BufReader, Read, Write},
    process::{Child, ChildStderr, ChildStdout, Command, ExitStatus},
    sync::mpsc::{self, Receiver},
    thread::{self, JoinHandle},
};

use anyhow::{Context, Error, Result};

/// An immediately guarded child and its pipe readers; readers never outlive cleanup.
pub(super) struct CompilerChild {
    child: Child,
    reaped: bool,
    output: Option<JoinHandle<Option<String>>>,
    errors: Option<JoinHandle<Option<String>>>,
}

impl CompilerChild {
    /// Owns an unreaped child before any pipe capture or thread startup is attempted.
    fn new(child: Child) -> Self {
        Self {
            child,
            reaped: false,
            output: None,
            errors: None,
        }
    }

    /// Arms cleanup immediately after spawn, before pipe capture or reader startup can fail.
    pub(super) fn spawn(
        command: &mut Command,
        out_log: Box<dyn Write + Send>,
        err_log: Box<dyn Write + Send>,
    ) -> Result<(Self, Receiver<String>)> {
        let mut running = Self::new(command.spawn().context("start the asset compiler")?);
        match running.start_readers(out_log, err_log) {
            Ok(received) => Ok((running, received)),
            Err(error) => Err(running.failed(error)),
        }
    }

    /// Starts both readers while retaining any already-started thread if the next step fails.
    fn start_readers(
        &mut self,
        out_log: Box<dyn Write + Send>,
        err_log: Box<dyn Write + Send>,
    ) -> Result<Receiver<String>> {
        let (stdout, stderr) = self.pipes()?;
        let (lines, received) = mpsc::channel();
        self.output = Some(
            thread::Builder::new()
                .spawn(move || {
                    copy_lines(stdout, out_log, |text| {
                        let _ = lines.send(text.to_owned());
                    })
                })
                .context("start the asset compiler output reader")?,
        );
        self.errors = Some(
            thread::Builder::new()
                .spawn(move || copy_lines(stderr, err_log, |_| {}))
                .context("start the asset compiler error reader")?,
        );
        Ok(received)
    }

    /// Takes both piped streams while the lifetime guard retains ownership of the child.
    fn pipes(&mut self) -> Result<(ChildStdout, ChildStderr)> {
        let stdout = self
            .child
            .stdout
            .take()
            .context("capture asset compiler output")?;
        let stderr = self
            .child
            .stderr
            .take()
            .context("capture asset compiler errors")?;
        Ok((stdout, stderr))
    }

    /// Disarms termination only once a successful poll has reaped the child.
    pub(super) fn poll_with(
        &mut self,
        wait: &mut impl FnMut(&mut Child) -> io::Result<Option<ExitStatus>>,
    ) -> Result<Option<ExitStatus>> {
        let status = wait(&mut self.child).context("wait for the asset compiler")?;
        self.reaped = status.is_some();
        Ok(status)
    }

    /// Stops an incomplete run before joining readers, retaining the original error's type.
    pub(super) fn finish(
        &mut self,
        result: Result<ExitStatus>,
    ) -> Result<(ExitStatus, Option<String>)> {
        match result {
            Err(error) => Err(self.failed(error)),
            Ok(status) => Ok((status, self.cleanup()?)),
        }
    }

    /// Retains the run's original error and attaches any failure to reap during cleanup.
    fn failed(&mut self, error: Error) -> Error {
        match self.cleanup() {
            Ok(_) => error,
            Err(cleanup) => error.context(format!("clean up the asset compiler: {cleanup:#}")),
        }
    }

    /// Attempts termination/reaping before waiting for either pipe reader to finish.
    fn cleanup(&mut self) -> Result<Option<String>> {
        let stopped = self.stop();
        let last_line = self.join_readers();
        stopped?;
        Ok(last_line)
    }

    /// Uses process-tree cancellation until a wait confirms the immediate child is reaped.
    fn stop(&mut self) -> Result<()> {
        if !self.reaped {
            cancel_child(&mut self.child)?;
            self.reaped = true;
        }
        Ok(())
    }

    /// Joins each reader once, returning the stderr tail for failed-exit diagnostics.
    fn join_readers(&mut self) -> Option<String> {
        if let Some(output) = self.output.take() {
            let _ = output.join();
        }
        self.errors
            .take()
            .and_then(|errors| errors.join().ok().flatten())
    }
}

impl Drop for CompilerChild {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

/// Stops the isolated setup process tree and reports a failure to reap its immediate child.
fn cancel_child(child: &mut Child) -> Result<()> {
    #[cfg(unix)]
    if let Some(pid) = i32::try_from(child.id())
        .ok()
        .and_then(rustix::process::Pid::from_raw)
    {
        let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
    }
    #[cfg(windows)]
    {
        let mut command = Command::new("taskkill");
        #[cfg(not(debug_assertions))]
        launcher_host::desktop::windows::hide_console(&mut command);
        let _ = command
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    child.wait().context("reap the stopped asset compiler")?;
    Ok(())
}

/// Copies whole lines without interleaving log writes and returns the last non-blank line.
fn copy_lines(
    output: impl Read,
    mut log: impl Write,
    mut each: impl FnMut(&str),
) -> Option<String> {
    let mut last = None;
    for line in BufReader::new(output).split(b'\n') {
        let Ok(mut line) = line else {
            break;
        };
        line.push(b'\n');
        let _ = log.write_all(&line);
        let text = String::from_utf8_lossy(&line).trim().to_owned();
        each(&text);
        if !text.is_empty() {
            last = Some(text);
        }
    }
    last
}

#[cfg(all(test, unix))]
mod tests;
