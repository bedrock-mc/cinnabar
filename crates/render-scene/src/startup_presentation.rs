//! Startup feedback shared by the renderer and the main-world lifecycle gate.

use bevy::prelude::Resource;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU8, Ordering},
};

/// Counts successful primary-window presentations whose submitted GPU work has completed.
#[derive(Clone, Default, Resource)]
pub struct StartupPresentation(Arc<Feedback>);

#[derive(Default)]
struct Feedback {
    frames: AtomicU8,
    content: Mutex<Content>,
}

#[derive(Default)]
struct Content {
    epoch: u64,
    active: bool,
    ready: bool,
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

    /// Starts a new native-surface lifetime, invalidating both pending and published feedback.
    pub fn replace_content_surface(&self, active: bool) {
        let mut content = self.0.content.lock().expect("startup content lock");
        content.epoch += 1;
        content.active = active;
        content.ready = false;
    }

    /// Identifies the currently usable native surface; hidden/offscreen views have none.
    pub fn content_epoch(&self) -> Option<u64> {
        let content = self.0.content.lock().expect("startup content lock");
        content.active.then_some(content.epoch)
    }

    /// Accepts GPU completion only for the surface that submitted the destination frame.
    pub fn record_content_frame(&self, epoch: u64) {
        let mut content = self.0.content.lock().expect("startup content lock");
        if content.active && content.epoch == epoch {
            content.ready = true;
        }
    }

    /// Clear-only frames and completions from retired surfaces cannot dismiss feedback.
    pub fn has_presented_content(&self) -> bool {
        let content = self.0.content.lock().expect("startup content lock");
        content.active && content.ready
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
        feedback.replace_content_surface(true);
        render_feedback.record_content_frame(feedback.content_epoch().unwrap());
        assert!(feedback.has_presented_content());
    }
}

#[cfg(test)]
mod surface_tests {
    use super::*;

    #[test]
    fn old_surface_callbacks_and_unconsumed_readiness_do_not_survive_replacement() {
        let feedback = StartupPresentation::default();
        feedback.replace_content_surface(true);
        let old = feedback.content_epoch().unwrap();
        feedback.record_content_frame(old);
        assert!(feedback.has_presented_content());
        feedback.replace_content_surface(false);
        assert!(!feedback.has_presented_content());
        assert_eq!(feedback.content_epoch(), None);
        feedback.record_content_frame(old);
        assert!(!feedback.has_presented_content());
        feedback.replace_content_surface(true);
        let new = feedback.content_epoch().unwrap();
        feedback.record_content_frame(old);
        assert!(!feedback.has_presented_content());
        feedback.record_content_frame(new);
        assert!(feedback.has_presented_content());
    }
}
