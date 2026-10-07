//! Prepares and launches helpers without holding the process lock during startup.

use super::{Dispatch, MAX_DISPATCH_IPC, MAX_STARTUP_IPC, Start, read_frame, write_frame};
use anyhow::{Result, bail, ensure};
use server_experience::{policy::*, runtime::Transaction};
use std::{
    path::PathBuf,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{Arc, Mutex, mpsc},
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
    pub(super) fn start(
        executable: PathBuf,
        prepare: impl FnOnce() -> Result<Start> + Send + 'static,
        requests: mpsc::Receiver<Dispatch>,
        responses: mpsc::SyncSender<Result<Transaction>>,
    ) -> Result<Self> {
        let process = Self::default();
        let worker = process.clone();
        std::thread::Builder::new()
            .name("experience-ipc".into())
            .spawn(move || {
                let startup = prepare().and_then(|startup| {
                    let (mut input, mut output) = worker.launch(executable)?;
                    write_frame(&mut input, &startup, MAX_STARTUP_IPC)?;
                    let transaction = read_frame(&mut output, MAX_HOST_OUTPUT)?;
                    Ok((input, output, transaction))
                });
                let (mut input, mut output, transaction) = match startup {
                    Ok(startup) => startup,
                    Err(error) => {
                        let _ = responses.send(Err(error));
                        return;
                    }
                };
                if responses.send(Ok(transaction)).is_err() {
                    return;
                }
                while let Ok(request) = requests.recv() {
                    let result = write_frame(&mut input, &request, MAX_DISPATCH_IPC)
                        .and_then(|()| read_frame(&mut output, MAX_HOST_OUTPUT));
                    let failed = result.is_err();
                    if responses.send(result).is_err() || failed {
                        return;
                    }
                }
            })?;
        Ok(process)
    }

    /// Publishes a launched child only if cancellation has not revoked the pending helper.
    fn launch(&self, executable: PathBuf) -> Result<(ChildStdin, ChildStdout)> {
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
            .stderr(Stdio::null())
            .spawn()?;
        let (Some(input), Some(output)) = (child.stdin.take(), child.stdout.take()) else {
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
        Ok((input, output))
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
