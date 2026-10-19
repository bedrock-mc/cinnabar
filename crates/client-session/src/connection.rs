//! Generation-fenced join progress independent of the menu and process owner.

use std::{path::PathBuf, process::ExitStatus, time::Instant};

use bridge::bridge_endpoint_exists;

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
    /// `core_exit` reports the per-session core's exit status once it has exited.
    pub fn poll(
        mut self,
        current_generation: u64,
        connecting: bool,
        core_exit: impl FnOnce() -> Option<ExitStatus>,
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
                    Err(error)
                        if self.local_world
                            || self
                                .address
                                .trim()
                                .starts_with(bridge::contract::GATHERING_TARGET_PREFIX) =>
                    {
                        JoinPoll::Failed {
                            message: format!("Could not open {}: {error}", self.address),
                            directory: None,
                        }
                    }
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
                let error = if let Some(status) = core_exit() {
                    format!("bedrock-core exited ({status}) before publishing its endpoint")
                } else if Instant::now() >= deadline {
                    "bedrock-core did not publish its endpoint".to_owned()
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

#[cfg(test)]
mod tests {
    use super::*;
    use bridge::CORE_START_TIMEOUT;
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
    fn launcher_failure_preserves_supported_remote_fallbacks_and_local_errors() {
        for address in [
            "fixture.invalid".to_owned(),
            format!("{}42", bridge::contract::REALM_TARGET_PREFIX),
            format!("{}1234", bridge::contract::FRIEND_TARGET_PREFIX),
        ] {
            let mut external = launcher_attempt(false, Err("no launcher response".into()));
            external.address = address;
            assert!(matches!(
                external.poll(7, true, || panic!("launcher has no session core")),
                JoinPoll::StartCore(_)
            ));
        }
        let local = launcher_attempt(true, Err("no launcher response".into()));
        let JoinPoll::Failed {
            message,
            directory: None,
        } = local.poll(7, true, || panic!("launcher has no session core"))
        else {
            panic!("local joins require their owning launcher core");
        };
        assert_eq!(
            message,
            "Could not open fixture.invalid: no launcher response"
        );
    }

    #[test]
    fn gathering_failure_preserves_the_launcher_error_without_starting_a_core() {
        for error in [
            "the launcher core did not answer",
            "launcher: join gathering: service unavailable",
        ] {
            let mut attempt = launcher_attempt(false, Err(error.into()));
            attempt.address = format!("{}fixture", bridge::contract::GATHERING_TARGET_PREFIX);
            let address = attempt.address.clone();
            let result = attempt.poll(7, true, || panic!("launcher has no session core"));
            let JoinPoll::Failed {
                message,
                directory: None,
            } = result
            else {
                panic!("a gathering must not fall back to an unresolved network address");
            };
            assert_eq!(message, format!("Could not open {address}: {error}"));
        }
    }

    #[test]
    fn gathering_waits_for_the_launcher_endpoint() {
        let (sender, receiver) = crossbeam_channel::bounded(1);
        let attempt = JoinAttempt::<()> {
            generation: 7,
            address: format!("{}fixture", bridge::contract::GATHERING_TARGET_PREFIX),
            auth_cache: None,
            local_world: false,
            stage: JoinStage::Launcher(receiver),
        };
        let JoinPoll::Pending(attempt) = attempt.poll(7, true, || panic!("no session core")) else {
            panic!("an unanswered launcher request must remain pending");
        };
        let socket_dir = PathBuf::from("gathering-fixture-endpoint");
        sender.send(Ok(socket_dir.clone())).unwrap();
        let JoinPoll::Ready {
            socket_dir: ready,
            directory: None,
        } = attempt.poll(7, true, || panic!("no session core"))
        else {
            panic!("a gathering must use the resolved launcher endpoint");
        };
        assert_eq!(ready, socket_dir);
    }

    #[test]
    fn disconnected_gathering_worker_does_not_start_a_core() {
        let (sender, receiver) = crossbeam_channel::bounded(1);
        drop(sender);
        let attempt = JoinAttempt::<()> {
            generation: 7,
            address: format!("{}fixture", bridge::contract::GATHERING_TARGET_PREFIX),
            auth_cache: None,
            local_world: false,
            stage: JoinStage::Launcher(receiver),
        };
        assert!(matches!(
            attempt.poll(7, true, || panic!("no session core")),
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
        let status = ExitStatus::default();
        let result = attempt.poll(7, true, || Some(status));
        let JoinPoll::Failed {
            directory: Some(_),
            message,
        } = &result
        else {
            panic!("an exited core fails the join keeping its directory");
        };
        assert!(
            message.contains(&format!("bedrock-core exited ({status}) before")),
            "{message}"
        );
        assert_eq!(releases.load(Ordering::SeqCst), 0);
        drop(result);
        assert_eq!(releases.load(Ordering::SeqCst), 1);
    }
}
