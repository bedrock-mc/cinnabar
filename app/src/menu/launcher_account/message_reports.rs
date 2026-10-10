//! Ordered inbox reports run independently of authentication and join polling.

use super::{Duration, MessageEvent, PathBuf, Receiver, Sender, thread};

const RETRY_INTERVAL: Duration = Duration::from_secs(15);

/// Starts a dedicated reporter; dropping the account link cancels queued retries.
pub(super) fn start(socket_dir: PathBuf, stop: Receiver<()>) -> Sender<MessageEvent> {
    let (sender, requests) = crossbeam_channel::unbounded();
    thread::spawn(move || {
        let Some(runtime) = super::runtime() else {
            return;
        };
        run(&requests, &stop, RETRY_INTERVAL, |event| {
            match runtime.block_on(bridge::report_message_event(&socket_dir, event)) {
                Ok(()) => true,
                Err(error) => {
                    bevy::log::warn!(%error, "inbox event failed; retaining for retry");
                    false
                }
            }
        });
    });
    sender
}

/// Retains a failed report until accepted, preserving read/delete order without a hot loop.
fn run(
    requests: &Receiver<MessageEvent>,
    stop: &Receiver<()>,
    retry: Duration,
    mut report: impl FnMut(&MessageEvent) -> bool,
) {
    loop {
        let event = crossbeam_channel::select_biased! {
            recv(stop) -> _ => return,
            recv(requests) -> event => match event {
                Ok(event) => event,
                Err(_) => return,
            },
        };
        while !report(&event) {
            if !super::wait(stop, retry) {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::{bounded, unbounded};

    /// Creates an interaction with enough identity to detect loss or reordering.
    fn event(kind: &str) -> MessageEvent {
        MessageEvent {
            event_type: kind.into(),
            instance_id: "message".into(),
            report_id: "report".into(),
            button_id: String::new(),
        }
    }

    #[test]
    fn failed_inbox_reports_retry_before_later_actions() {
        let (sender, requests) = unbounded();
        let (_alive, stop) = bounded(0);
        let click = event("Click");
        let delete = event("Delete");
        sender.send(click.clone()).unwrap();
        sender.send(delete.clone()).unwrap();
        drop(sender);
        let mut attempts = Vec::new();
        run(&requests, &stop, Duration::ZERO, |event| {
            attempts.push(event.clone());
            attempts.len() > 1
        });
        assert_eq!(attempts, [click.clone(), click, delete]);
    }

    #[test]
    fn failed_inbox_report_stops_retrying_when_account_link_drops() {
        let (sender, requests) = unbounded();
        let (alive, stop) = bounded(0);
        let mut alive = Some(alive);
        sender.send(event("Delete")).unwrap();
        let mut attempts = 0;
        run(&requests, &stop, RETRY_INTERVAL, |_| {
            attempts += 1;
            drop(alive.take());
            false
        });
        assert_eq!(attempts, 1);
    }
}

#[cfg(all(test, unix))]
mod polling_tests;
