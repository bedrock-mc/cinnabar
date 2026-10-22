//! Startup feedback shared by the renderer and the main-world lifecycle gate.

use bevy::prelude::Resource;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, Ordering},
};

/// Counts successful primary-window presentations whose submitted GPU work has completed.
#[derive(Clone, Default, Resource)]
pub struct StartupPresentation(Arc<Feedback>);

#[derive(Default)]
struct Feedback {
    frames: AtomicU8,
    content: AtomicBool,
}

impl StartupPresentation {
    /// The first presented frame and two more establish that rendering kept making progress.
    pub const REQUIRED_FRAMES: u8 = 3;

    /// Records one render-completion callback, retaining only the small startup frame count.
    pub fn record_completed_frame(&self) {
        let _ = self
            .0
            .frames
            .try_update(Ordering::Release, Ordering::Relaxed, |frames| {
                (frames < Self::REQUIRED_FRAMES).then_some(frames + 1)
            });
    }

    /// Records a completed frame containing a fully drawn destination interface.
    pub fn record_content_frame(&self) {
        self.0.content.store(true, Ordering::Release);
    }

    /// Clear-only frames cannot dismiss startup feedback.
    pub fn has_presented_content(&self) -> bool {
        self.0.content.load(Ordering::Acquire)
    }

    /// Whether enough presented frames have completed to confirm client startup.
    pub fn is_complete(&self) -> bool {
        self.0.frames.load(Ordering::Acquire) >= Self::REQUIRED_FRAMES
    }

    /// Whether the first primary-window presentation has completed its submitted GPU work.
    pub fn has_presented(&self) -> bool {
        self.0.frames.load(Ordering::Acquire) > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_requires_the_first_presentation_and_following_frames() {
        let feedback = StartupPresentation::default();
        let render_feedback = feedback.clone();
        assert!(!feedback.is_complete());
        assert!(!feedback.has_presented());
        assert!(!feedback.has_presented_content());
        for _ in 1..StartupPresentation::REQUIRED_FRAMES {
            render_feedback.record_completed_frame();
            assert!(feedback.has_presented());
            assert!(!feedback.is_complete());
        }
        render_feedback.record_completed_frame();
        assert!(feedback.is_complete());
        render_feedback.record_completed_frame();
        assert!(feedback.is_complete());
        assert!(!feedback.has_presented_content());
        render_feedback.record_content_frame();
        assert!(feedback.has_presented_content());
    }
}
