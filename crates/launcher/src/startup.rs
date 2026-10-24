//! Asset-independent startup feedback shared by native hosts.

use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// Work already entered by the client, in monotonic startup order.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    #[default]
    LoadingAssets,
    PreparingInterface,
    OpeningWindow,
    WaitingForContent,
}

/// Original colors and user-facing labels, available before any game carrier.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Appearance {
    pub background: [u8; 4],
    pub foreground: [u8; 4],
    pub accent: [u8; 4],
    pub layout: Layout,
    loading_assets: String,
    preparing_interface: String,
    opening_window: String,
    waiting_for_content: String,
}

/// Layout shared by the desktop canvas and Android view, independent of device density.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    pub design_size: [f32; 2],
    pub minimum_scale: f32,
    pub logo_width: f32,
    pub side_margin: f32,
    pub logo_center_y: f32,
    pub label_top: f32,
    pub label_size: f32,
    pub indicator_top: f32,
    pub dot_size: f32,
    pub dot_spacing: f32,
    pub step_millis: u64,
    pub frame_millis: u64,
}

/// Loads the same tracked startup specification consumed by Android packaging.
pub fn appearance() -> &'static Appearance {
    static VALUE: OnceLock<Appearance> = OnceLock::new();
    VALUE.get_or_init(|| {
        serde_json::from_str(include_str!("../../../assets/startup-screen.json"))
            .expect("the tracked startup appearance parses")
    })
}

impl Stage {
    /// Describes work, without claiming a fraction or estimating its remaining duration.
    pub fn label(self) -> &'static str {
        let labels = appearance();
        match self {
            Self::LoadingAssets => &labels.loading_assets,
            Self::PreparingInterface => &labels.preparing_interface,
            Self::OpeningWindow => &labels.opening_window,
            Self::WaitingForContent => &labels.waiting_for_content,
        }
    }
}

/// Parent-to-window messages; end-of-file also retires the loading window.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Message {
    Progress { stage: Stage },
    Ready,
}

/// Feedback from the window, distinguished from a failure to open it.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Event {
    Shown,
    Cancelled,
}

/// A terminal handoff cannot be undone by a delayed progress message.
#[derive(Default)]
pub struct Model {
    stage: Stage,
    finished: bool,
}

impl Model {
    /// Applies a newer phase or terminal content-ready notification.
    pub fn apply(&mut self, message: Message) {
        if self.finished {
            return;
        }
        match message {
            Message::Progress { stage } => self.stage = self.stage.max(stage),
            Message::Ready => self.finished = true,
        }
    }

    /// Current work, without depending on a display or renderer.
    pub const fn stage(&self) -> Stage {
        self.stage
    }

    /// Whether the destination frame is ready or the owner has gone away.
    pub const fn finished(&self) -> bool {
        self.finished
    }

    /// Retires this launch after cancellation or transport closure.
    pub fn finish(&mut self) {
        self.finished = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delayed_phase_messages_cannot_rewind_or_revive_a_launch() {
        let mut model = Model::default();
        model.apply(Message::Progress {
            stage: Stage::OpeningWindow,
        });
        model.apply(Message::Progress {
            stage: Stage::LoadingAssets,
        });
        assert_eq!(model.stage(), Stage::OpeningWindow);
        assert!(!model.finished());
        model.apply(Message::Ready);
        model.apply(Message::Progress {
            stage: Stage::WaitingForContent,
        });
        assert!(model.finished());
        assert_eq!(model.stage(), Stage::OpeningWindow);
    }

    #[test]
    fn closing_the_owner_is_terminal_and_never_implies_content_readiness() {
        let mut model = Model::default();
        model.finish();
        model.apply(Message::Progress {
            stage: Stage::OpeningWindow,
        });
        assert!(model.finished());
        assert_eq!(model.stage(), Stage::LoadingAssets);
    }
}
