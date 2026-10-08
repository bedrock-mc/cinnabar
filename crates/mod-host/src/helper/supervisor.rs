//! Prepares and launches helpers without holding the process lock during startup, and forwards
//! their stderr to the client's log.

use super::{
    Dispatch, MAX_DISPATCH_IPC, MAX_LOG_LINE_BYTES, MAX_LOG_LINES_PER_SECOND, MAX_REPLY_IPC,
    MAX_STARTUP_IPC, Reply, Start, read_frame, write_frame,
};
use anyhow::{Result, bail, ensure};
use server_experience::policy::*;
use std::{
    io::{BufRead, BufReader, Read},
    path::PathBuf,
    process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio},
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};

#[derive(Default)]
struct State {
    child: Option<Child>,
    revoked: bool,
}

#[derive(Clone, Default)]
pub(super) struct Process(Arc<Mutex<State>>);

impl Process {
    /// Starts the supervisor and reports preparation, launch and IPC failures through polling.
    /// The helper's stderr goes to `log`, cut and rate-limited.
    pub(super) fn start(
        executable: PathBuf,
        prepare: impl FnOnce() -> Result<Start> + Send + 'static,
        requests: mpsc::Receiver<Dispatch>,
        responses: mpsc::SyncSender<Result<Reply>>,
        log: mpsc::SyncSender<String>,
    ) -> Result<Self> {
        let process = Self::default();
        let worker = process.clone();
        std::thread::Builder::new()
            .name("experience-ipc".into())
            .spawn(move || {
                let startup = prepare().and_then(|startup| {
                    let (mut input, mut output, stderr) = worker.launch(executable)?;
                    std::thread::Builder::new()
                        .name("experience-stderr".into())
                        .spawn(move || forward_stderr(stderr, &log))?;
                    write_frame(&mut input, &startup, MAX_STARTUP_IPC)?;
                    let reply = read_frame(&mut output, MAX_REPLY_IPC)?;
                    Ok((input, output, reply))
                });
                let (mut input, mut output, reply) = match startup {
                    Ok(startup) => startup,
                    Err(error) => {
                        let _ = responses.send(Err(error));
                        return;
                    }
                };
                // A failed start is the helper's last word.
                let started = matches!(reply, Reply::Committed { .. });
                if responses.send(Ok(reply)).is_err() || !started {
                    return;
                }
                while let Ok(request) = requests.recv() {
                    let result = write_frame(&mut input, &request, MAX_DISPATCH_IPC)
                        .and_then(|()| read_frame(&mut output, MAX_REPLY_IPC));
                    let failed = result.is_err();
                    if responses.send(result).is_err() || failed {
                        return;
                    }
                }
            })?;
        Ok(process)
    }

    /// Publishes a launched child only if cancellation has not revoked the pending helper.
    fn launch(&self, executable: PathBuf) -> Result<(ChildStdin, ChildStdout, ChildStderr)> {
        ensure!(!self.0.lock().unwrap().revoked, "helper startup revoked");
        let mut command = Command::new(executable);
        command
            .arg("server-helper")
            .env_clear()
            .env(DEVELOPER_ENV, "1");
        // The granted scope may name the developer loopback media origin, which validates only
        // with its CA variable present.
        if let Some(ca) = std::env::var_os(server_experience::fetch::DEVELOPER_MEDIA_CA_ENV) {
            command.env(server_experience::fetch::DEVELOPER_MEDIA_CA_ENV, ca);
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let (Some(input), Some(output), Some(stderr)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            let _ = child.kill();
            let _ = child.wait();
            bail!("missing helper pipes");
        };
        let mut state = self.0.lock().unwrap();
        if state.revoked {
            drop(state);
            let _ = child.kill();
            let _ = child.wait();
            bail!("helper startup revoked");
        }
        state.child = Some(child);
        Ok((input, output, stderr))
    }

    /// Cancels a pending launch or kills its published process without waiting for startup.
    pub(super) fn cancel(&self) {
        let mut state = self.0.lock().unwrap();
        state.revoked = true;
        if let Some(child) = &mut state.child {
            let _ = child.kill();
        }
    }

    /// Takes the child under a short lock and waits for its exit on a separate thread.
    pub(super) fn reap(&self) {
        let process = self.clone();
        let _ = std::thread::Builder::new()
            .name("experience-reap".into())
            .spawn(move || {
                let child = process.0.lock().unwrap().child.take();
                if let Some(mut child) = child {
                    let _ = child.wait();
                }
            });
    }
}

/// Sends each line of a helper's stderr to `lines` until it ends: each cut to
/// [`MAX_LOG_LINE_BYTES`], at most [`MAX_LOG_LINES_PER_SECOND`] a second, the others counted in a
/// line of their own. A full channel drops the line.
pub(super) fn forward_stderr(stderr: impl Read, lines: &mpsc::SyncSender<String>) {
    let mut reader = BufReader::new(stderr);
    let mut window = Instant::now();
    let (mut sent, mut dropped) = (0, 0);
    let summary = |dropped: usize| format!("{dropped} more helper stderr lines dropped");
    loop {
        let mut line = Vec::new();
        let limit = MAX_LOG_LINE_BYTES as u64 + 1;
        match (&mut reader).take(limit).read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        if line.last() != Some(&b'\n') && line.len() > MAX_LOG_LINE_BYTES {
            skip_line(&mut reader);
        }
        while matches!(line.last(), Some(b'\n' | b'\r')) {
            line.pop();
        }
        let mut text = String::from_utf8_lossy(&line).into_owned();
        text.truncate(text.floor_char_boundary(MAX_LOG_LINE_BYTES));
        if window.elapsed() >= Duration::from_secs(1) {
            if dropped > 0 {
                let _ = lines.try_send(summary(dropped));
            }
            (window, sent, dropped) = (Instant::now(), 0, 0);
        }
        if sent < MAX_LOG_LINES_PER_SECOND {
            let _ = lines.try_send(text);
            sent += 1;
        } else {
            dropped += 1;
        }
    }
    if dropped > 0 {
        let _ = lines.try_send(summary(dropped));
    }
}

/// Reads past the rest of the current line.
fn skip_line(reader: &mut impl BufRead) {
    let mut rest = Vec::new();
    loop {
        rest.clear();
        match reader.take(4096).read_until(b'\n', &mut rest) {
            Ok(0) | Err(_) => return,
            Ok(_) if rest.last() == Some(&b'\n') => return,
            Ok(_) => {}
        }
    }
}
