use std::{io, path::PathBuf, thread, time::Duration};

use bridge::LocalWorld;
use crossbeam_channel::Sender;

use super::manager::Manager;

/// Bounds one `local_world.v1` call.
const REPORT_TIMEOUT: Duration = Duration::from_secs(3);
/// Spaces retries while the core cannot be reached.
const RETRY_DELAY: Duration = Duration::from_millis(250);

/// Keeps one core's route to the open local world current until dropped.
pub struct CoreLink {
    _stop: Sender<()>,
}

impl CoreLink {
    /// Reports the manager's open world to the core at `socket_dir` now and after every change.
    pub fn spawn(manager: Manager, socket_dir: PathBuf) -> io::Result<Self> {
        let (stop, stopped) = crossbeam_channel::bounded::<()>(0);
        let changes = manager.subscribe();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        thread::Builder::new()
            .name("local-world-core".to_owned())
            .spawn(move || {
                let mut reported: Option<LocalWorld> = None;
                loop {
                    let report = manager.core_report();
                    let mut pending = false;
                    if reported.as_ref() != Some(&report) {
                        match runtime.block_on(deliver(&socket_dir, &report)) {
                            Ok(()) => reported = Some(report),
                            Err(error) => {
                                tracing::debug!("local world route not delivered yet: {error}");
                                pending = true;
                            }
                        }
                    }
                    let retry = if pending {
                        crossbeam_channel::after(RETRY_DELAY)
                    } else {
                        crossbeam_channel::never()
                    };
                    crossbeam_channel::select! {
                        recv(stopped) -> _ => return,
                        recv(changes) -> _ => {}
                        recv(retry) -> _ => {}
                    }
                }
            })?;
        Ok(Self { _stop: stop })
    }
}

async fn deliver(socket_dir: &std::path::Path, report: &LocalWorld) -> Result<(), String> {
    match tokio::time::timeout(REPORT_TIMEOUT, bridge::set_local_world(socket_dir, report)).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(error.to_string()),
        Err(_) => Err("the core did not answer".to_owned()),
    }
}
