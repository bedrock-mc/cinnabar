//! Microsoft sign-in and Xbox signup browser handoff, independent of the desktop launcher.

/// Whether the current sign-in prompt reached the system browser.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BrowserState {
    #[default]
    Waiting,
    Opening,
    Opened,
    Failed,
}

const SIGN_IN_PAGE: &str = "https://www.microsoft.com/link?otc=";

/// Tracks one automatic handoff per sign-in prompt and explicit user retries.
#[derive(Debug, Default)]
pub struct SignInLink {
    target: Option<String>,
    attempted: std::collections::HashSet<String>,
    state: BrowserState,
    revision: u64,
}

impl SignInLink {
    /// Dispatches a pre-filled URL once per code, or again on an explicit press.
    pub fn open(&mut self, code: &str, explicit: bool, dispatch: impl FnOnce(String, u64) -> bool) {
        self.open_target(
            code,
            valid_code(code).then(|| format!("{SIGN_IN_PAGE}{code}")),
            explicit,
            dispatch,
        );
    }

    /// Dispatches a validated HTTPS Xbox signup URL once, or again on an explicit press.
    pub fn open_signup(
        &mut self,
        uri: &str,
        explicit: bool,
        dispatch: impl FnOnce(String, u64) -> bool,
    ) {
        self.open_target(uri, Some(uri.to_owned()), explicit, dispatch);
    }

    /// Orders handoffs for either prompt, retaining the one automatic attempt per target.
    fn open_target(
        &mut self,
        target: &str,
        url: Option<String>,
        explicit: bool,
        dispatch: impl FnOnce(String, u64) -> bool,
    ) {
        if self.attempted.contains(target) && !explicit {
            return;
        }
        self.attempted.insert(target.to_owned());
        self.target = Some(target.to_owned());
        self.revision += 1;
        let Some(url) = url else {
            self.state = BrowserState::Failed;
            return;
        };
        self.state = if dispatch(url, self.revision) {
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

    /// Returns the handoff status for this device code or signup URL, hiding stale attempts.
    pub fn state_for(&self, target: &str) -> BrowserState {
        if self.target.as_deref() == Some(target) {
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

    #[test]
    fn xbox_signup_keeps_its_signed_url_and_opens_once() {
        let uri = "https://sisu.xboxlive.com/signup?signature=fixture";
        let mut link = SignInLink::default();
        let mut device_revision = 0;
        link.open("DEVICE", false, |_, revision| {
            device_revision = revision;
            true
        });
        let mut signup_revision = 0;
        link.open_signup(uri, false, |url, revision| {
            assert_eq!(url, uri);
            signup_revision = revision;
            true
        });
        link.complete(device_revision, false);
        assert_eq!(link.state_for(uri), BrowserState::Opening);
        link.complete(signup_revision, true);
        link.open_signup(uri, false, |_, _| panic!("repeated signup handoff"));
        assert_eq!(link.state_for(uri), BrowserState::Opened);
    }
}
