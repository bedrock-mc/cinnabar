//! Starts the installed game and reports its lifetime; the launcher stays as a quiet supervisor.

use anyhow::{Context, Result, ensure};
use launcher::desktop::{GameExit, SUPERVISED_ENV};
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    process::{Command, ExitStatus, Stdio},
    sync::{Arc, Mutex},
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GameState {
    Stopped {
        #[serde(skip_serializing_if = "Option::is_none")]
        #[cfg_attr(test, ts(optional))]
        last_exit: Option<LastExit>,
    },
    Starting,
    Running {
        started_at: String,
    },
}

/// How the last game ended; a typed exit reason is never a crash.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LastExit {
    pub code: Option<i32>,
    pub crashed: bool,
}

/// A server to join directly once the game starts.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ServerTarget {
    pub address: String,
    pub port: u16,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LaunchOptions {
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub server: Option<ServerTarget>,
}

/// Classifies an exit status into what the launcher shows and the typed reason, if any.
pub fn ended(code: Option<i32>) -> (LastExit, Option<GameExit>) {
    let reason = code.and_then(GameExit::from_code);
    (
        LastExit {
            code,
            crashed: reason.is_none(),
        },
        reason,
    )
}

/// The game's command line: supervised, and joining `server` when one was chosen.
pub fn command(executable: &Path, server: Option<&ServerTarget>) -> Command {
    let mut command = Command::new(executable);
    command
        .env(SUPERVISED_ENV, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(server) = server {
        let address = if server.port == 0 {
            server.address.clone()
        } else {
            format!("{}:{}", server.address, server.port)
        };
        command.arg("--address").arg(address);
    }
    command
}

/// The one game this launcher supervises at a time.
pub struct Supervisor {
    state: Mutex<GameState>,
    listener: Box<dyn Fn(&GameState) + Send + Sync>,
}

impl Supervisor {
    pub fn new(listener: impl Fn(&GameState) + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(GameState::Stopped { last_exit: None }),
            listener: Box::new(listener),
        })
    }

    pub fn state(&self) -> GameState {
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()
    }

    pub fn running(&self) -> bool {
        !matches!(self.state(), GameState::Stopped { .. })
    }

    /// Starts `command` and calls `exited` from a watcher thread once the game ends.
    pub fn launch(
        self: &Arc<Self>,
        mut command: Command,
        started_at: String,
        exited: impl FnOnce(LastExit, Option<GameExit>) + Send + 'static,
    ) -> Result<()> {
        {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            ensure!(
                matches!(*state, GameState::Stopped { .. }),
                "Cinnabar is already running"
            );
            *state = GameState::Starting;
        }
        self.publish();
        let mut child = match command.spawn().context("start Cinnabar") {
            Ok(child) => child,
            Err(error) => {
                self.set(GameState::Stopped {
                    last_exit: Some(LastExit {
                        code: None,
                        crashed: true,
                    }),
                });
                return Err(error);
            }
        };
        self.set(GameState::Running { started_at });
        let supervisor = Arc::clone(self);
        std::thread::spawn(move || {
            let code = child
                .wait()
                .ok()
                .and_then(|status: ExitStatus| status.code());
            let (last_exit, reason) = ended(code);
            supervisor.set(GameState::Stopped {
                last_exit: Some(last_exit),
            });
            exited(last_exit, reason);
        });
        Ok(())
    }

    fn set(&self, next: GameState) {
        *self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()) = next;
        self.publish();
    }

    fn publish(&self) {
        (self.listener)(&self.state());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_exits_are_not_crashes_and_other_endings_are() {
        assert_eq!(
            ended(Some(0)),
            (
                LastExit {
                    code: Some(0),
                    crashed: false
                },
                Some(GameExit::Quit)
            )
        );
        let code = GameExit::ReturnForUpdate.code();
        assert_eq!(ended(Some(code)).1, Some(GameExit::ReturnForUpdate));
        assert!(!ended(Some(code)).0.crashed);
        for crash in [Some(1), Some(101), None] {
            assert!(ended(crash).0.crashed);
            assert_eq!(ended(crash).1, None);
        }
    }

    #[test]
    fn direct_joins_pass_the_address_and_experiences_pass_their_id() {
        let args = |server: Option<ServerTarget>| {
            command(Path::new("cinnabar"), server.as_ref())
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        };
        assert!(args(None).is_empty());
        assert_eq!(
            args(Some(ServerTarget {
                address: "play.example.test".into(),
                port: 19132
            })),
            ["--address", "play.example.test:19132"]
        );
        let experience = format!("{}abc", launcher::menu::EXPERIENCE_ADDRESS_PREFIX);
        assert_eq!(
            args(Some(ServerTarget {
                address: experience.clone(),
                port: 0
            })),
            ["--address", experience.as_str()]
        );
        assert!(
            command(Path::new("cinnabar"), None)
                .get_envs()
                .any(|(name, value)| name == SUPERVISED_ENV && value.is_some())
        );
    }

    #[test]
    fn states_serialize_with_the_launcher_contract_names() {
        assert_eq!(
            serde_json::to_value(GameState::Stopped {
                last_exit: Some(LastExit {
                    code: None,
                    crashed: true
                })
            })
            .unwrap(),
            serde_json::json!({ "kind": "stopped", "lastExit": { "code": null, "crashed": true } })
        );
        assert_eq!(
            serde_json::to_value(GameState::Running {
                started_at: "t".into()
            })
            .unwrap(),
            serde_json::json!({ "kind": "running", "startedAt": "t" })
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_supervisor_reports_the_whole_lifetime_and_refuses_a_second_game() {
        let states = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&states);
        let supervisor = Supervisor::new(move |state| seen.lock().unwrap().push(state.clone()));
        let (sender, receiver) = std::sync::mpsc::channel();
        // The game runs until the test closes its stdin, then quits for an update.
        let (reader, writer) = std::io::pipe().unwrap();
        let mut game = Command::new("/bin/sh");
        game.args(["-c", "read line; exit 75"]).stdin(reader);
        supervisor
            .launch(game, "t".into(), move |exit, reason| {
                sender.send((exit, reason)).unwrap();
            })
            .unwrap();
        assert!(supervisor.running());
        assert!(
            supervisor
                .launch(Command::new("/bin/true"), "t".into(), |_, _| {})
                .is_err()
        );
        drop(writer);
        let (exit, reason) = receiver.recv().unwrap();
        assert_eq!(reason, Some(GameExit::ReturnForUpdate));
        assert!(!exit.crashed);
        assert!(!supervisor.running());
        let states = states.lock().unwrap();
        assert!(matches!(states[0], GameState::Starting));
        assert!(matches!(states[1], GameState::Running { .. }));
        assert!(matches!(
            states.last(),
            Some(GameState::Stopped {
                last_exit: Some(LastExit { code: Some(75), .. })
            })
        ));
    }
}
