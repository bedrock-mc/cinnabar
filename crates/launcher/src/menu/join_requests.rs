//! Discord "Ask to Join" requests waiting for the host's answer, oldest first.

use std::{collections::VecDeque, time::Duration};

/// How long Discord keeps a join request open before closing it on its side.
pub const LIFETIME: Duration = Duration::from_secs(30);

/// The toast and popup line naming who asked; vanilla has no string for it.
pub fn title(name: &str) -> String {
    format!("{name} wants to join")
}

/// The toast's second line, naming the bound Open Notification key. Vanilla's invite line
/// promises the key accepts, but this one opens Accept and Decline.
pub fn respond_hint(key: &str) -> String {
    format!("Press {key} to respond")
}

/// One Discord user waiting for an answer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JoinRequest {
    pub user_id: u64,
    pub name: String,
    /// When Discord last delivered it, on the caller's monotonic clock.
    arrived: Duration,
}

impl JoinRequest {
    /// When Discord closes it unanswered, on the same clock as its arrival.
    pub fn closes_at(&self) -> Duration {
        self.arrived.saturating_add(LIFETIME)
    }
}

/// The open requests in the order they first arrived.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct JoinRequests {
    pending: VecDeque<JoinRequest>,
}

impl JoinRequests {
    /// Queues a request. Asking again keeps the user's place under their newest name and
    /// restarts the lifetime.
    pub fn push(&mut self, user_id: u64, name: String, now: Duration) {
        if let Some(waiting) = self
            .pending
            .iter_mut()
            .find(|request| request.user_id == user_id)
        {
            waiting.name = name;
            waiting.arrived = now;
            return;
        }
        self.pending.push_back(JoinRequest {
            user_id,
            name,
            arrived: now,
        });
    }

    /// Drops requests Discord has already closed.
    pub fn expire(&mut self, now: Duration) {
        self.pending.retain(|request| now < request.closes_at());
    }

    /// The oldest request still open once [`Self::expire`] has run.
    pub fn current(&self) -> Option<&JoinRequest> {
        self.pending.front()
    }

    /// Answers the current request, giving `(user_id, accept)` to send to Discord.
    pub fn answer(&mut self, accept: bool) -> Option<(u64, bool)> {
        let request = self.pending.pop_front()?;
        Some((request.user_id, accept))
    }

    pub fn clear(&mut self) {
        self.pending.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECOND: Duration = Duration::from_secs(1);

    fn names(requests: &JoinRequests) -> Vec<(u64, &str)> {
        requests
            .pending
            .iter()
            .map(|request| (request.user_id, request.name.as_str()))
            .collect()
    }

    #[test]
    fn a_user_asking_again_keeps_their_place_under_their_newest_name() {
        let mut requests = JoinRequests::default();
        requests.push(1, "old".into(), Duration::ZERO);
        requests.push(2, "second".into(), SECOND);
        requests.push(1, "new".into(), 2 * SECOND);
        assert_eq!(names(&requests), [(1, "new"), (2, "second")]);
    }

    #[test]
    fn requests_close_a_lifetime_after_their_latest_arrival() {
        let mut requests = JoinRequests::default();
        requests.push(1, "first".into(), Duration::ZERO);
        requests.push(2, "second".into(), 10 * SECOND);
        requests.expire(LIFETIME - SECOND);
        assert_eq!(requests.current().map(|request| request.user_id), Some(1));
        // The oldest lapses and the next becomes current.
        requests.expire(LIFETIME);
        assert_eq!(requests.current().map(|request| request.user_id), Some(2));
        assert_eq!(
            requests.current().map(JoinRequest::closes_at),
            Some(10 * SECOND + LIFETIME)
        );
        // Asking again restarts the lifetime.
        requests.push(2, "second".into(), LIFETIME);
        requests.expire(LIFETIME + 10 * SECOND + LIFETIME / 2);
        assert_eq!(requests.current().map(|request| request.user_id), Some(2));
        requests.expire(2 * LIFETIME);
        assert!(requests.current().is_none());
    }

    #[test]
    fn answers_go_to_the_oldest_request_in_arrival_order() {
        let mut requests = JoinRequests::default();
        requests.push(7, "a".into(), Duration::ZERO);
        requests.push(9, "b".into(), SECOND);
        assert_eq!(
            requests.current().map(|request| request.name.as_str()),
            Some("a")
        );
        assert_eq!(requests.answer(true), Some((7, true)));
        assert_eq!(
            requests.current().map(|request| request.name.as_str()),
            Some("b")
        );
        assert_eq!(requests.answer(false), Some((9, false)));
        assert_eq!(requests.answer(true), None);
    }

    #[test]
    fn clearing_forgets_every_request() {
        let mut requests = JoinRequests::default();
        requests.push(1, "a".into(), Duration::ZERO);
        assert!(requests.current().is_some());
        requests.clear();
        assert!(requests.current().is_none());
        assert_eq!(requests.answer(true), None);
    }

    #[test]
    fn the_join_popup_waits_behind_other_popups() {
        let mut view = super::super::MenuView::new(true, "Host".into());
        view.join_request = Some("friend".into());
        assert_eq!(view.join_request_prompt(), Some("friend"));
        assert!(view.popup_open());
        view.dialog = Some(super::super::MenuDialog::Exit);
        assert_eq!(view.join_request_prompt(), None);
        view.dialog = None;
        view.connecting = true;
        view.feeds.server_trust = Some(super::super::ServerTrustPrompt::default());
        assert_eq!(view.join_request_prompt(), None);
    }
}
