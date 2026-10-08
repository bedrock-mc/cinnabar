//! Device-code browser handoff, independent of the desktop launcher.

/// Whether the current device code reached the system browser.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BrowserState {
    #[default]
    Waiting,
    Opening,
    Opened,
    Failed,
}

const SIGN_IN_PAGE: &str = "https://www.microsoft.com/link?otc=";

/// Tracks one automatic handoff per device code and explicit user retries.
#[derive(Debug, Default)]
pub struct SignInLink {
    code: Option<String>,
    attempted: std::collections::HashSet<String>,
    state: BrowserState,
    revision: u64,
}

impl SignInLink {
    /// Dispatches a pre-filled URL once per code, or again on an explicit press.
    pub fn open(&mut self, code: &str, explicit: bool, dispatch: impl FnOnce(String, u64) -> bool) {
        if self.attempted.contains(code) && !explicit {
            return;
        }
        self.attempted.insert(code.to_owned());
        self.code = Some(code.to_owned());
        self.revision += 1;
        if !valid_code(code) {
            self.state = BrowserState::Failed;
            return;
        }
        self.state = if dispatch(format!("{SIGN_IN_PAGE}{code}"), self.revision) {
            BrowserState::Opening
        } else {
            BrowserState::Failed
        };
    }

    /// Applies a completed handoff only while its request is still current.
    pub fn complete(&mut self, revision: u64, opened: bool) {
        if revision == self.revision {
            self.state = if opened {
                BrowserState::Opened
            } else {
                BrowserState::Failed
            };
        }
    }

    /// Returns the handoff status for this code, hiding stale attempts.
    pub fn state_for(&self, code: &str) -> BrowserState {
        if self.code.as_deref() == Some(code) {
            self.state
        } else {
            BrowserState::Waiting
        }
    }
}

/// Only device-code characters may enter the pre-filled query parameter.
fn valid_code(code: &str) -> bool {
    !code.is_empty()
        && code
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_open_is_once_and_explicit_press_reopens_after_failure() {
        let mut link = SignInLink::default();
        let mut requests = Vec::new();
        link.open("TEST-CODE", false, |url, id| {
            requests.push((url, id));
            true
        });
        assert_eq!(requests[0].0, format!("{SIGN_IN_PAGE}TEST-CODE"));
        assert_eq!(link.state_for("TEST-CODE"), BrowserState::Opening);
        link.complete(requests[0].1, false);
        for _ in 0..100 {
            link.open("TEST-CODE", false, |_, _| panic!("repeated automatic open"));
        }
        assert_eq!(link.state_for("TEST-CODE"), BrowserState::Failed);
        link.open("TEST-CODE", true, |url, id| {
            requests.push((url, id));
            true
        });
        link.complete(requests[1].1, true);
        assert_eq!(link.state_for("TEST-CODE"), BrowserState::Opened);
        assert_eq!(requests.len(), 2);
    }

    #[test]
    fn new_codes_ignore_old_completion_and_failed_dispatch() {
        let mut link = SignInLink::default();
        let mut old = 0;
        link.open("OLD", false, |_, id| {
            old = id;
            true
        });
        assert_eq!(link.state_for("NEW"), BrowserState::Waiting);
        link.open("NEW", false, |_, _| false);
        link.complete(old, true);
        assert_eq!(link.state_for("NEW"), BrowserState::Failed);
    }

    #[test]
    fn a_stale_code_reappearing_after_a_retry_does_not_auto_open() {
        let mut link = SignInLink::default();
        link.open("FIRST", false, |_, _| true);
        link.open("SECOND", false, |_, _| true);
        link.open("FIRST", false, |_, _| panic!("stale code auto-opened"));
        let mut reopened = false;
        link.open("FIRST", true, |_, _| {
            reopened = true;
            true
        });
        assert!(reopened);
    }

    #[test]
    fn invalid_codes_never_reach_the_opener() {
        let mut link = SignInLink::default();
        for code in [
            "",
            "abc&otc=other",
            "https://example.test",
            "abc def",
            "abc\n",
            "é",
        ] {
            link.open(code, true, |_, _| panic!("unsafe browser URL"));
            assert_eq!(link.state_for(code), BrowserState::Failed);
        }
    }
}
