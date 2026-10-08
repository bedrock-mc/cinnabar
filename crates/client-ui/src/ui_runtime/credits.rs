//! Session-bound End credits and their single completion acknowledgement.

use super::{FormTransportError, UiRuntime};

pub const CREDITS_SCREEN: &str = "credits.credits_screen";
pub const CREDITS_SCROLL_PIXELS_PER_SECOND: f64 = 20.0;
pub const CREDITS_FADE_MILLIS: u64 = 3_000;
pub const CREDITS_SKIP_VISIBLE_MILLIS: u64 = 5_000;

#[derive(Clone, Debug)]
pub struct CreditsSession {
    pub runtime_id: u64,
    pub sequence: u64,
    pub opened_millis: u64,
    scroll_pixels: f64,
    observed_millis: u64,
    fade_started_millis: Option<u64>,
    skip_until_millis: Option<u64>,
}

impl CreditsSession {
    pub const fn scroll_pixels(&self) -> f64 {
        self.scroll_pixels
    }

    pub fn skip_visible(&self, now: u64) -> bool {
        self.skip_until_millis.is_some_and(|until| now < until)
    }

    pub fn cover_alpha(&self, now: u64) -> f32 {
        match self.fade_started_millis {
            Some(start) => now.saturating_sub(start) as f32 / CREDITS_FADE_MILLIS as f32,
            None => {
                1.0 - now.saturating_sub(self.opened_millis) as f32 / CREDITS_FADE_MILLIS as f32
            }
        }
        .clamp(0.0, 1.0)
    }
}

#[derive(Clone, Debug, Default)]
pub struct CreditsState {
    active: Option<CreditsSession>,
    pending: Option<u64>,
    last_sequence: Option<u64>,
}

impl CreditsState {
    pub fn active(&self) -> Option<&CreditsSession> {
        self.active.as_ref()
    }
    pub fn owns_input(&self) -> bool {
        self.active.is_some() || self.pending.is_some()
    }

    pub fn open(&mut self, runtime_id: u64, sequence: u64, now: u64) -> bool {
        if runtime_id == 0
            || self
                .last_sequence
                .is_some_and(|previous| sequence <= previous)
            || self.owns_input()
        {
            return false;
        }
        self.last_sequence = Some(sequence);
        self.active = Some(CreditsSession {
            runtime_id,
            sequence,
            opened_millis: now,
            scroll_pixels: 0.0,
            observed_millis: now,
            fade_started_millis: None,
            skip_until_millis: None,
        });
        true
    }

    /// The first selection exposes Skip; selecting again closes the credits.
    pub fn select(&mut self, now: u64, cancel: bool) {
        if cancel
            && self
                .active
                .as_ref()
                .is_some_and(|active| active.skip_visible(now))
        {
            self.skip(now);
        } else if let Some(active) = self.active.as_mut() {
            active.skip_until_millis = Some(now.saturating_add(CREDITS_SKIP_VISIBLE_MILLIS));
        }
    }

    pub fn skip(&mut self, _now: u64) {
        if let Some(active) = self.active.take() {
            self.pending = Some(active.runtime_id);
        }
    }

    /// `scroll_end` starts the screen's fade; completion remains queued until accepted.
    pub fn observe(&mut self, now: u64, content_finished: bool) {
        let Some(active) = self.active.as_mut() else {
            return;
        };
        let elapsed = now.saturating_sub(active.observed_millis);
        active.observed_millis = active.observed_millis.max(now);
        active.scroll_pixels += elapsed as f64 * CREDITS_SCROLL_PIXELS_PER_SECOND / 1_000.0;
        if content_finished {
            active.fade_started_millis.get_or_insert(now);
        }
        if active
            .fade_started_millis
            .is_some_and(|start| now.saturating_sub(start) >= CREDITS_FADE_MILLIS)
        {
            self.pending = Some(active.runtime_id);
            self.active = None;
        }
    }

    pub fn flush(
        &mut self,
        current_runtime_id: Option<u64>,
        send: impl FnOnce(protocol::Packet) -> Result<(), FormTransportError>,
    ) -> Result<bool, FormTransportError> {
        if self.pending.is_none() {
            return Ok(false);
        }
        let Some(runtime_id) = current_runtime_id.filter(|id| *id != 0) else {
            return Ok(false);
        };
        match send(protocol::credits_finished_packet(runtime_id)) {
            Ok(()) => {
                self.pending = None;
                Ok(true)
            }
            Err(FormTransportError::Full) => Err(FormTransportError::Full),
            Err(FormTransportError::Closed) => {
                self.pending = None;
                Err(FormTransportError::Closed)
            }
        }
    }
}

impl UiRuntime {
    pub fn credits(&self) -> &CreditsState {
        &self.credits
    }
    pub fn credits_mut(&mut self) -> &mut CreditsState {
        &mut self.credits
    }
    pub fn credits_player_name(&self) -> &str {
        &self.chat_source_name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credits_scroll_then_fade_complete_once_with_queue_backpressure() {
        let mut credits = CreditsState::default();
        assert!(credits.open(41, 7, 100));
        credits.observe(1_100, false);
        assert_eq!(credits.active().unwrap().scroll_pixels(), 20.0);
        credits.observe(1_000, false);
        assert_eq!(credits.active().unwrap().scroll_pixels(), 20.0);
        credits.observe(1_200, true);
        credits.observe(4_199, true);
        assert!(credits.active().is_some());
        credits.observe(4_200, true);
        assert!(credits.owns_input());
        assert_eq!(
            credits.flush(Some(41), |_| Err(FormTransportError::Full)),
            Err(FormTransportError::Full)
        );
        assert!(credits.owns_input());
        let mut packets = Vec::new();
        assert_eq!(
            credits.flush(Some(41), |packet| {
                packets.push(packet);
                Ok(())
            }),
            Ok(true)
        );
        assert_eq!(
            credits.flush(Some(41), |_| panic!("duplicate completion")),
            Ok(false)
        );
        assert_eq!(packets.len(), 1);
        assert!(!credits.owns_input());
    }

    #[test]
    fn skip_visibility_is_timed_and_duplicate_open_does_not_restart_the_poem() {
        let mut credits = CreditsState::default();
        credits.open(41, 7, 100);
        credits.select(110, true);
        assert!(credits.active().unwrap().skip_visible(111));
        credits.select(120, true);
        assert!(!credits.open(41, 8, 130));
        credits.observe(3_120, false);
        assert!(credits.active().is_none());
        credits.flush(Some(41), |_| Ok(())).unwrap();
        assert!(!credits.open(41, 7, 4_000));
        assert!(credits.open(41, 9, 4_000));
    }

    #[test]
    fn changing_sessions_retires_credits_and_the_unsent_completion() {
        let mut runtime = UiRuntime::new(1);
        runtime.credits_mut().open(41, 7, 100);
        runtime.credits_mut().skip(110);
        runtime.begin_session(2);
        assert!(!runtime.credits().owns_input());
        assert_eq!(
            runtime
                .credits_mut()
                .flush(Some(72), |_| panic!("old session response")),
            Ok(false)
        );
        assert!(runtime.credits_mut().open(72, 1, 120));
    }

    #[test]
    fn session_replacement_clears_local_credits_name_until_new_identity() {
        let mut runtime = UiRuntime::new(1);
        runtime.set_chat_identity("PreviousPlayer".into(), "previous-account".into());
        runtime.begin_session(1);
        assert_eq!(runtime.credits_player_name(), "PreviousPlayer");
        runtime.begin_session(2);
        runtime.credits_mut().open(72, 1, 120);
        assert_eq!(
            runtime.credits_player_name(),
            "",
            "a new local actor must not inherit the previous player's poem name"
        );
        assert!(runtime.chat_xuid.is_empty());
        runtime.set_chat_identity("CurrentPlayer".into(), "current-account".into());
        assert_eq!(runtime.credits_player_name(), "CurrentPlayer");
    }

    #[test]
    fn credits_completion_waits_for_the_current_local_actor_identity() {
        let mut credits = CreditsState::default();
        credits.open(41, 7, 0);
        credits.skip(100);
        assert_eq!(credits.flush(None, |_| panic!("actor absent")), Ok(false));
        assert!(credits.owns_input());
        let mut completed = None;
        credits
            .flush(Some(72), |packet| {
                completed = Some(packet);
                Ok(())
            })
            .unwrap();
        assert_eq!(completed, Some(protocol::credits_finished_packet(72)));
        assert!(!credits.owns_input());
    }
}
