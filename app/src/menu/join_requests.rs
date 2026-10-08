//! The host's open Discord join requests: a standing toast names the oldest one in play, vanilla's
//! Open Notification key or a press on the toast opens its popup over the pause screen, and the
//! answers wait here for the Discord adapter to send.

use std::{collections::VecDeque, ops::DerefMut, sync::Arc, time::Duration};

use bevy::prelude::{Res, ResMut};
use client_ui::ui_runtime::UiRuntime;
use launcher::menu::{
    join_requests::{self, JoinRequests},
    settings_options::{OPEN_NOTIFICATION_KEY, key_name},
};
use semantic_input::Action;
use ui::{StandingToast, ToastPress};

use super::{MenuAction, MenuRuntime};
use crate::semantic_controls::SemanticInputSnapshot;

#[derive(Debug, Default)]
pub(super) struct JoinRequestUi {
    requests: JoinRequests,
    /// `(user_id, accept)` answers, oldest first.
    replies: VecDeque<(u64, bool)>,
}

impl MenuRuntime {
    /// Queues a request Discord delivered at `now`.
    pub(crate) fn push_join_request(&mut self, user_id: u64, name: String, now: Duration) {
        self.join_requests.requests.push(user_id, name, now);
    }

    /// Drops the requests Discord has closed by `now`.
    pub(crate) fn expire_join_requests(&mut self, now: Duration) {
        self.join_requests.requests.expire(now);
    }

    /// Forgets every request and unsent answer once Discord is gone.
    pub(crate) fn clear_join_requests(&mut self) {
        self.join_requests.requests.clear();
        self.join_requests.replies.clear();
    }

    /// The next answer for Discord.
    pub(crate) fn take_join_reply(&mut self) -> Option<(u64, bool)> {
        self.join_requests.replies.pop_front()
    }

    /// A press on the join request toast opens the pause screen, which shows the popup.
    pub(crate) fn open_join_requests(&mut self) {
        if self.join_requests.requests.current().is_some() {
            self.open_pause();
        }
    }

    /// Stands the oldest request's toast while playing until Discord closes it, and slides the
    /// toast out once that request is answered, closed or shown as the popup. Touches `runtime`
    /// only on a change.
    pub(crate) fn sync_join_toast(
        &self,
        runtime: &mut impl DerefMut<Target = UiRuntime>,
        now: Duration,
    ) {
        let millis = |at: Duration| u64::try_from(at.as_millis()).unwrap_or(u64::MAX);
        let now = millis(now);
        let standing = runtime.hud().standing_toast();
        let Some(request) = self
            .join_requests
            .requests
            .current()
            .filter(|_| !self.visible)
        else {
            if standing.is_some_and(|toast| toast.until_millis > now) {
                runtime.retire_standing_toast(now);
            }
            return;
        };
        let until = millis(request.closes_at());
        if standing.is_some_and(|toast| toast.id == request.user_id && toast.until_millis == until)
        {
            return;
        }
        let hint = self
            .settings_options
            .named_key_control(OPEN_NOTIFICATION_KEY)
            .map(|key| join_requests::respond_hint(&key_name(key)))
            .unwrap_or_default();
        runtime.stand_toast(StandingToast {
            id: request.user_id,
            title: Arc::from(join_requests::title(&request.name)),
            message: Arc::from(hint),
            press: ToastPress::JoinRequests,
            since_millis: now,
            until_millis: until,
        });
    }

    /// Who sent the oldest open request, for the view.
    pub(super) fn join_request_view(&self) -> Option<String> {
        self.join_requests
            .requests
            .current()
            .map(|request| request.name.clone())
    }

    /// Whether the popup is up: the menu shows and no other popup outranks it.
    pub(super) fn join_request_prompted(&self) -> bool {
        self.visible
            && self.sign_in_focus().is_none()
            && self.dialog.is_none()
            && !(self.is_connecting() && self.feeds.server_trust.is_some())
            && self.join_requests.requests.current().is_some()
    }

    pub(super) fn answer_join_request(&mut self, accept: bool) {
        if let Some(reply) = self.join_requests.requests.answer(accept) {
            self.join_requests.replies.push_back(reply);
        }
        self.focused = 0;
    }

    /// Keyboard and gamepad order while the popup is up.
    pub(super) fn join_request_focus_actions(&self) -> Option<Vec<MenuAction>> {
        self.join_request_prompted().then(|| {
            vec![
                MenuAction::JoinRequest(true),
                MenuAction::JoinRequest(false),
            ]
        })
    }
}

/// Vanilla's Open Notification key opens the oldest join request's popup from play.
pub(crate) fn open_join_requests_from_key(
    input: Res<SemanticInputSnapshot>,
    mut menu: ResMut<MenuRuntime>,
) {
    if input.phase(Action::InteractWithToast).pressed {
        menu.open_join_requests();
    }
}

#[cfg(test)]
mod tests;
