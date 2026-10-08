//! Generation-fenced join progress independent of the menu and process owner.

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

/// How long a freshly spawned core has to publish its bridge endpoint.
pub const CORE_START_TIMEOUT: Duration = Duration::from_secs(5);

/// A join still provisioning while the connecting screen shows; polled each frame.
#[derive(Debug)]
pub struct JoinAttempt<D> {
    pub generation: u64,
    pub address: String,
    pub auth_cache: Option<PathBuf>,
    pub local_world: bool,
    pub stage: JoinStage<D>,
}

/// Endpoint provisioning retains the directory guard until its owning core stops.
#[derive(Debug)]
pub enum JoinStage<D> {
    /// The launcher core selecting the target on a worker.
    Launcher(crossbeam_channel::Receiver<Result<PathBuf, String>>),
    /// A per-session core that must publish its endpoint by the deadline.
    Core {
        socket_dir: PathBuf,
        directory: D,
        deadline: Instant,
    },
}

/// One bounded poll asks the application only for the next process or publication action.
#[derive(Debug)]
pub enum JoinPoll<D> {
    Retired,
    Pending(JoinAttempt<D>),
    Ready {
        socket_dir: PathBuf,
        directory: Option<D>,
    },
    StartCore(JoinAttempt<D>),
    Failed {
        message: String,
        directory: Option<D>,
    },
}

impl<D> JoinAttempt<D> {
    /// Advances the existing join in order, before the app inserts a replacement network handle.
    pub fn poll(
        mut self,
        current_generation: u64,
        connecting: bool,
        core_exited: impl FnOnce() -> bool,
    ) -> JoinPoll<D> {
        if self.generation != current_generation || !connecting {
            return JoinPoll::Retired;
        }
        match self.stage {
            JoinStage::Launcher(ref receiver) => {
                let selected = match receiver.try_recv() {
                    Err(crossbeam_channel::TryRecvError::Empty) => return JoinPoll::Pending(self),
                    Err(crossbeam_channel::TryRecvError::Disconnected) => {
                        Err("the launcher core did not answer".to_owned())
                    }
                    Ok(selected) => selected,
                };
                match selected {
                    Ok(socket_dir) => JoinPoll::Ready {
                        socket_dir,
                        directory: None,
                    },
                    Err(error) if self.local_world => JoinPoll::Failed {
                        message: format!("Could not open {}: {error}", self.address),
                        directory: None,
                    },
                    Err(_) => JoinPoll::StartCore(self),
                }
            }
            JoinStage::Core {
                socket_dir,
                directory,
                deadline,
            } => {
                if bridge_endpoint_exists(&socket_dir) {
                    return JoinPoll::Ready {
                        socket_dir,
                        directory: Some(directory),
                    };
                }
                let error = if core_exited() {
                    "bedrock-core exited before publishing its endpoint"
                } else if Instant::now() >= deadline {
                    "bedrock-core did not publish its endpoint"
                } else {
                    self.stage = JoinStage::Core {
                        socket_dir,
                        directory,
                        deadline,
                    };
                    return JoinPoll::Pending(self);
                };
                JoinPoll::Failed {
                    message: format!(
                        "Could not start {}: {error} at {}",
                        self.address,
                        socket_dir.display()
                    ),
                    directory: Some(directory),
                }
            }
        }
    }
}

/// Applies the bridge transport's platform-specific endpoint readiness rule.
pub fn bridge_endpoint_exists(directory: &Path) -> bool {
    let endpoint = protocol::bridge_endpoint_path(directory);
    if cfg!(windows) {
        endpoint.is_file()
    } else {
        endpoint.exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    /// Counts directory release without creating a core process or a socket.
    #[derive(Debug)]
    struct Directory(Arc<AtomicUsize>);

    impl Drop for Directory {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// Supplies a completed launcher reply for a single pending generation.
    fn launcher_attempt(local_world: bool, result: Result<PathBuf, String>) -> JoinAttempt<()> {
        let (sender, receiver) = crossbeam_channel::bounded(1);
        sender.send(result).unwrap();
        JoinAttempt {
            generation: 7,
            address: "fixture.invalid".into(),
            auth_cache: None,
            local_world,
            stage: JoinStage::Launcher(receiver),
        }
    }

    #[test]
    fn retired_generation_does_not_poll_the_process_or_start_transport() {
        let attempt = launcher_attempt(false, Ok(PathBuf::from("fixture-endpoint")));
        assert!(matches!(
            attempt.poll(8, true, || panic!("retired process was polled")),
            JoinPoll::Retired
        ));
    }

    #[test]
    fn launcher_failure_falls_back_only_for_external_servers() {
        let external = launcher_attempt(false, Err("no launcher response".into()));
        assert!(matches!(
            external.poll(7, true, || panic!("launcher has no session core")),
            JoinPoll::StartCore(_)
        ));
        let local = launcher_attempt(true, Err("no launcher response".into()));
        assert!(matches!(
            local.poll(7, true, || panic!("launcher has no session core")),
            JoinPoll::Failed {
                directory: None,
                ..
            }
        ));
    }

    #[test]
    fn failed_core_keeps_its_directory_until_the_app_stops_the_process() {
        let releases = Arc::new(AtomicUsize::new(0));
        let attempt = JoinAttempt {
            generation: 7,
            address: "fixture.invalid".into(),
            auth_cache: None,
            local_world: false,
            stage: JoinStage::Core {
                socket_dir: PathBuf::from("missing-session-endpoint-for-unit-test"),
                directory: Directory(Arc::clone(&releases)),
                deadline: Instant::now() + CORE_START_TIMEOUT,
            },
        };
        let result = attempt.poll(7, true, || true);
        assert!(matches!(
            result,
            JoinPoll::Failed {
                directory: Some(_),
                ..
            }
        ));
        assert_eq!(releases.load(Ordering::SeqCst), 0);
        drop(result);
        assert_eq!(releases.load(Ordering::SeqCst), 1);
    }
}
