//! The core's server trust question: sending the player's answer off the frame thread, and polling
//! a per-session core, which no account link watches, for its question while it prepares a join.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crossbeam_channel::{RecvTimeoutError, Sender, bounded};
use launcher::menu::view::ServerTrustPrompt;
use protocol::launcher_control::{answer_server_trust, poll_events};

/// How often a per-session core's question is polled, matching the join's event polling.
const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Delivery attempts before an answer is given up and its prompt shown again.
const ANSWER_ATTEMPTS: u32 = 3;

/// Answers trust prompt `id`, retrying briefly; `undelivered` runs when the answer never reached
/// the core, so the caller can show the still-pending question again.
pub(crate) fn send(
    socket_dir: PathBuf,
    id: u64,
    trusted: bool,
    undelivered: impl FnOnce() + Send + 'static,
) {
    thread::spawn(move || {
        if let Some(runtime) = runtime() {
            for attempt in 1..=ANSWER_ATTEMPTS {
                match runtime.block_on(answer_server_trust(&socket_dir, id, trusted)) {
                    Ok(_) => return,
                    Err(error) if attempt == ANSWER_ATTEMPTS => {
                        bevy::log::warn!(%error, "server trust answer was not delivered");
                    }
                    Err(_) => thread::sleep(POLL_INTERVAL),
                }
            }
        }
        undelivered();
    });
}

fn runtime() -> Option<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()
}

/// Where the menu reads a pending trust question and sends its answer.
pub(crate) trait TrustSource {
    fn prompt(&self) -> Option<ServerTrustPrompt>;
    fn answer(&self, id: u64, trusted: bool);
}

#[derive(Default)]
struct Watched {
    prompt: Option<ServerTrustPrompt>,
    answered: Option<u64>,
}

/// Polls one per-session core's events for its trust question until dropped.
pub(crate) struct SessionTrust {
    socket_dir: PathBuf,
    watched: Arc<Mutex<Watched>>,
    _stop: Sender<()>,
}

impl std::fmt::Debug for SessionTrust {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionTrust")
            .field("socket_dir", &self.socket_dir)
            .finish_non_exhaustive()
    }
}

impl SessionTrust {
    pub(crate) fn watch(socket_dir: PathBuf) -> Self {
        let watched = Arc::new(Mutex::new(Watched::default()));
        let (stop, stopped) = bounded::<()>(0);
        let (shared, dir) = (Arc::clone(&watched), socket_dir.clone());
        thread::spawn(move || {
            let Some(runtime) = runtime() else {
                return;
            };
            while let Err(RecvTimeoutError::Timeout) = stopped.recv_timeout(POLL_INTERVAL) {
                let Ok(events) = runtime.block_on(poll_events(&dir)) else {
                    continue;
                };
                let prompt = events.server_trust.map(|prompt| ServerTrustPrompt {
                    id: prompt.id,
                    url: prompt.url,
                    from_session_core: true,
                });
                shared
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner())
                    .prompt = prompt;
            }
        });
        Self {
            socket_dir,
            watched,
            _stop: stop,
        }
    }

    fn with<T>(&self, read: impl FnOnce(&mut Watched) -> T) -> T {
        read(
            &mut self
                .watched
                .lock()
                .unwrap_or_else(|poison| poison.into_inner()),
        )
    }
}

impl TrustSource for SessionTrust {
    fn prompt(&self) -> Option<ServerTrustPrompt> {
        self.with(|watched| {
            let prompt = watched.prompt.as_ref()?;
            (watched.answered != Some(prompt.id)).then(|| prompt.clone())
        })
    }

    fn answer(&self, id: u64, trusted: bool) {
        self.with(|watched| watched.answered = Some(id));
        let watched = Arc::clone(&self.watched);
        send(self.socket_dir.clone(), id, trusted, move || {
            let mut watched = watched.lock().unwrap_or_else(|poison| poison.into_inner());
            if watched.answered == Some(id) {
                watched.answered = None;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // An answer that cannot reach its core gives the question back instead of hiding it.
    #[test]
    fn an_undelivered_answer_shows_the_question_again() {
        let dir = std::env::temp_dir().join(format!("cinnabar-trust-{}", std::process::id()));
        let trust = SessionTrust::watch(dir);
        let prompt = ServerTrustPrompt {
            id: 2,
            url: "http://127.0.0.1:19132".into(),
            from_session_core: true,
        };
        trust.with(|watched| watched.prompt = Some(prompt.clone()));
        trust.answer(2, true);
        assert!(
            trust.prompt().is_none(),
            "the answered question stayed visible"
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while trust.with(|watched| watched.answered.is_some()) {
            assert!(
                std::time::Instant::now() < deadline,
                "the question never came back"
            );
            thread::sleep(Duration::from_millis(20));
        }
        trust.with(|watched| watched.prompt = Some(prompt.clone()));
        assert_eq!(trust.prompt(), Some(prompt));
    }
}
