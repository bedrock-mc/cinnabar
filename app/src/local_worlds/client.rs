use std::{io, path::PathBuf, thread, time::Duration};

use crossbeam_channel::{Receiver, Sender, unbounded};
use protocol::world_control::{self as control, BridgeError};

use super::model::{Effect, Event};

const POLL_DELAY: Duration = Duration::from_millis(250);
const UNAVAILABLE: &str = "Local world service is unavailable";

/// Runs [`Effect`]s against the core's control endpoint on a worker thread, in order.
pub(crate) struct WorldsClient {
    effects: Sender<Effect>,
    events: Receiver<Event>,
}

impl WorldsClient {
    /// Starts the worker; it exits when the client is dropped.
    pub(crate) fn spawn(socket_dir: PathBuf) -> io::Result<Self> {
        let (effects, effect_rx) = unbounded::<Effect>();
        let (event_tx, events) = unbounded::<Event>();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        thread::Builder::new()
            .name("local-worlds".to_owned())
            .spawn(move || {
                while let Ok(effect) = effect_rx.recv() {
                    if matches!(effect, Effect::PollStatus | Effect::PollPrefs) {
                        thread::sleep(POLL_DELAY);
                    }
                    let preferences = matches!(
                        effect,
                        Effect::LoadPrefs | Effect::PollPrefs | Effect::SetPrefs { .. }
                    );
                    let event =
                        runtime
                            .block_on(execute(&socket_dir, effect))
                            .map(|event| match event {
                                Event::Failed(message) if preferences => {
                                    Event::FailedPrefs(message)
                                }
                                event => event,
                            });
                    if let Some(event) = event
                        && event_tx.send(event).is_err()
                    {
                        break;
                    }
                }
            })?;
        Ok(Self { effects, events })
    }

    pub(crate) fn send(&self, effect: Effect) {
        let _ = self.effects.send(effect);
    }

    pub(crate) fn drain(&self) -> Vec<Event> {
        self.events.try_iter().collect()
    }
}

/// Short user-facing text for a failed request; never includes local paths.
pub(super) fn describe(error: &BridgeError) -> String {
    match error {
        BridgeError::ControlRpc { message, .. } => message.clone(),
        _ => UNAVAILABLE.to_owned(),
    }
}

fn failed(error: BridgeError) -> Event {
    Event::Failed(describe(&error))
}

async fn prefs_event(update: control::PrefsUpdate, dir: &std::path::Path) -> Event {
    control::local_worlds_prefs(dir, &update)
        .await
        .map_or_else(failed, |(prefs, status)| Event::Prefs(prefs, status))
}

async fn execute(dir: &std::path::Path, effect: Effect) -> Option<Event> {
    match effect {
        Effect::List => Some(
            control::list_worlds(dir)
                .await
                .map_or_else(failed, Event::Listed),
        ),
        Effect::Create(world) => Some(
            control::create_world(dir, &world)
                .await
                .map_or_else(failed, Event::Created),
        ),
        Effect::Delete(id) => {
            let result = control::delete_world(dir, &id).await;
            Some(result.map_or_else(failed, |()| Event::Deleted(id)))
        }
        Effect::Update { id, update } => Some(
            control::update_world(dir, &id, &update)
                .await
                .map_or_else(failed, Event::Updated),
        ),
        Effect::Open(id) => Some(match control::open_world(dir, &id).await {
            Ok(status) => Event::Status(status),
            Err(BridgeError::ControlRpc { code, .. }) if code == control::CODE_EULA_REQUIRED => {
                Event::EulaRequired
            }
            Err(error) => failed(error),
        }),
        Effect::LoadPrefs | Effect::PollPrefs => {
            Some(prefs_event(control::PrefsUpdate::default(), dir).await)
        }
        Effect::SetPrefs {
            dismiss_docker_prompt,
            redetect,
        } => {
            let update = control::PrefsUpdate {
                docker_prompt_dismissed: dismiss_docker_prompt.then_some(true),
                redetect,
            };
            Some(prefs_event(update, dir).await)
        }
        Effect::AcceptEula => Some(
            control::accept_bds_eula(dir)
                .await
                .map_or_else(failed, |_| Event::EulaAccepted),
        ),
        Effect::OpenUrl(_) => None,
        Effect::PollStatus => Some(
            control::world_status(dir)
                .await
                .map_or_else(failed, Event::Status),
        ),
        Effect::Close => {
            let _ = control::close_world(dir).await;
            None
        }
        Effect::SetPaused(paused) => {
            let _ = control::set_world_paused(dir, paused).await;
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rpc_messages_pass_through_and_other_errors_hide_detail() {
        let rpc = BridgeError::ControlRpc {
            code: -32010,
            message: "world not found".to_owned(),
        };
        assert_eq!(describe(&rpc), "world not found");
        let io = BridgeError::Io(io::Error::other("open /Users/x/secret: denied"));
        assert_eq!(describe(&io), UNAVAILABLE);
        assert_eq!(describe(&BridgeError::ControlClosed), UNAVAILABLE);
    }

    #[test]
    fn unreachable_core_reports_failure_event() {
        let dir = tempfile_dir();
        let client = WorldsClient::spawn(dir).expect("spawn worker");
        client.send(Effect::List);
        test_time::eventually_within(Duration::from_secs(5), "an event from the worker", || {
            let events = client.drain();
            if let Some(event) = events.first() {
                assert!(matches!(event, Event::Failed(message) if message == UNAVAILABLE));
                true
            } else {
                false
            }
        });
    }

    fn tempfile_dir() -> PathBuf {
        std::env::temp_dir().join(format!(
            "cinnabar-local-worlds-absent-{}",
            std::process::id()
        ))
    }
}
