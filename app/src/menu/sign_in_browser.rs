//! Asynchronous handoff to the system's default browser.

use crossbeam_channel::{Receiver, bounded};
use launcher::menu::sign_in::{BrowserState, SignInLink};

#[derive(Debug, Default)]
pub(super) struct SignInBrowser {
    link: SignInLink,
    pending: Option<Receiver<(u64, bool)>>,
}

impl SignInBrowser {
    /// Starts a handoff without waiting for the desktop, using an injectable opener.
    pub(super) fn open(&mut self, code: &str, explicit: bool, opener: fn(&str) -> bool) {
        self.poll();
        self.link.open(code, explicit, |url, revision| {
            let (sender, receiver) = bounded(1);
            let spawned = std::thread::Builder::new()
                .name("sign-in-browser".into())
                .spawn(move || {
                    let _ = sender.send((revision, opener(&url)));
                })
                .is_ok();
            self.pending = spawned.then_some(receiver);
            spawned
        });
    }

    /// Collects the OS handoff result without blocking the menu frame.
    pub(super) fn poll(&mut self) {
        if let Some(receiver) = &self.pending
            && let Ok((revision, opened)) = receiver.try_recv()
        {
            self.link.complete(revision, opened);
            self.pending = None;
        }
    }

    /// Returns the status belonging to the visible auth prompt.
    pub(super) fn state(&self, auth: &super::AuthState) -> BrowserState {
        match auth {
            super::AuthState::AwaitingCode { code, .. } => self.link.state_for(code),
            _ => BrowserState::Waiting,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn injected_opener_reports_failure_and_an_explicit_press_can_succeed() {
        let mut browser = SignInBrowser::default();
        let auth = super::super::AuthState::AwaitingCode {
            uri: "https://example.invalid".into(),
            code: "TEST-CODE".into(),
        };
        for (opener, expected) in [
            ((|_: &str| false) as fn(&str) -> bool, BrowserState::Failed),
            ((|_: &str| true) as fn(&str) -> bool, BrowserState::Opened),
        ] {
            browser.open("TEST-CODE", true, opener);
            {
                let mut completion = crossbeam_channel::Select::new();
                completion.recv(
                    browser
                        .pending
                        .as_ref()
                        .expect("injected opener dispatched"),
                );
                completion.ready();
            }
            browser.poll();
            assert_eq!(browser.state(&auth), expected);
            browser.open("TEST-CODE", false, |_| panic!("automatic retry"));
            assert_eq!(browser.state(&auth), expected);
        }
        assert_eq!(
            browser.state(&super::super::AuthState::Authenticated),
            BrowserState::Waiting
        );
    }
}
