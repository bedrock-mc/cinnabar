//! Actual client frame cadence measured against monotonic wall time.
use std::time::{Duration, Instant};
#[derive(Default)]
pub(super) struct FrameRate {
    start: Option<Instant>,
    frames: u32,
    measured: Option<f32>,
}
impl FrameRate {
    /// Samples only completed callback frames and updates the one-second observation window.
    pub(super) fn sample(&mut self, now: Instant) -> Option<f32> {
        let Some(start) = self.start else {
            self.start = Some(now);
            return None;
        };
        self.frames = self.frames.saturating_add(1);
        let elapsed = now.duration_since(start);
        if elapsed >= Duration::from_secs(1) {
            self.measured = Some(self.frames as f32 / elapsed.as_secs_f32());
            self.frames = 0;
            self.start = Some(now);
        }
        self.measured
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cadence_uses_actual_elapsed_time_including_stalls() {
        let mut rate = FrameRate::default();
        let start = Instant::now();
        assert_eq!(rate.sample(start), None);
        for frame in 1..60 {
            assert_eq!(rate.sample(start + Duration::from_millis(frame * 16)), None);
        }
        assert_eq!(rate.sample(start + Duration::from_secs(1)), Some(60.));
        assert_eq!(rate.sample(start + Duration::from_secs(3)), Some(0.5));
    }
}

/// Projects only bounded newest received text; oversized server fields degrade to a UTF-8 prefix.
pub(super) fn chat_snapshot(
    ui: &client_ui::ui_runtime::UiRuntime,
) -> Vec<mod_host::ObservedChatMessage> {
    let mut messages: Vec<_> = ui
        .chat()
        .messages()
        .iter()
        .rev()
        .take(ui::mod_hud::MAX_OBSERVED_CHAT_MESSAGES)
        .map(|m| {
            let mut end = m.message.len().min(ui::mod_hud::MAX_OBSERVED_TEXT_BYTES);
            while !m.message.is_char_boundary(end) {
                end -= 1;
            }
            mod_host::ObservedChatMessage {
                sequence: m.fifo_sequence,
                text: m.message[..end].to_owned(),
            }
        })
        .collect();
    messages.reverse();
    messages
}
