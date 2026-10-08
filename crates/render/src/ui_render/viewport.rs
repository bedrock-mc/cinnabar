//! Retained viewport bytes and the clock used only by visible item glint.

use super::{glint::UiGlintSettings, shader::UiViewportUniform};

/// Keeps the last uniform submitted to the renderer's existing viewport buffer.
#[derive(Default)]
pub(super) struct ViewportUploads {
    last: Option<UiViewportUniform>,
    #[cfg(test)]
    pub(super) writes: usize,
}

impl ViewportUploads {
    /// Samples an active glint clock and dispatches changed viewport bytes without allocating.
    pub(super) fn upload(
        &mut self,
        size: [u32; 2],
        glint: UiGlintSettings,
        animated: bool,
        clock: impl FnOnce() -> f32,
        write: impl FnOnce(&UiViewportUniform),
    ) -> bool {
        let time_seconds = if animated && glint.strength != 0.0 && glint.speed != 0.0 {
            glint.animation_seconds(clock())
        } else {
            0.0
        };
        let uniform = UiViewportUniform {
            viewport_size: [size[0] as f32, size[1] as f32],
            time_seconds,
            glint_strength: glint.strength,
        };
        if self.last.as_ref() == Some(&uniform) {
            return false;
        }
        #[cfg(test)]
        {
            self.writes += 1;
        }
        write(&uniform);
        self.last = Some(uniform);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An unchanged static UI neither samples its unused clock nor dispatches another upload.
    #[test]
    fn unused_viewport_clock_does_not_dispatch_uploads() {
        let mut uploads = ViewportUploads::default();
        let mut dispatched = 0;
        uploads.upload(
            [1920, 1080],
            UiGlintSettings::default(),
            false,
            || panic!("static UI sampled the animation clock"),
            |_| dispatched += 1,
        );
        let before = crate::alloc_count::thread_allocations();
        for _ in 0..10 {
            uploads.upload(
                [1920, 1080],
                UiGlintSettings::default(),
                false,
                || panic!("static UI sampled the animation clock"),
                |_| dispatched += 1,
            );
        }
        let allocations = crate::alloc_count::thread_allocations() - before;
        assert_eq!(allocations, 0);
        assert_eq!(dispatched, 1, "unchanged viewport uploaded");
        assert_eq!(uploads.writes, dispatched);
    }

    /// Resizing and strength changes invalidate the retained uniform once each.
    #[test]
    fn viewport_resize_and_settings_publish_once() {
        let mut uploads = ViewportUploads::default();
        let mut observed = Vec::new();
        for (size, strength) in [
            ([800, 600], 1.0),
            ([1024, 768], 1.0),
            ([1024, 768], 0.5),
            ([1024, 768], 0.5),
        ] {
            let glint = UiGlintSettings {
                strength,
                speed: 1.0,
            };
            uploads.upload(
                size,
                glint,
                false,
                || panic!("unused clock"),
                |value| observed.push(*value),
            );
        }
        assert_eq!(observed.len(), 3);
        assert_eq!(observed[1].viewport_size, [1024.0, 768.0]);
        assert_eq!(observed[2].glint_strength, 0.5);
    }

    /// Glint uses the current global phase while the retained UI publication stays unchanged.
    #[test]
    fn active_glint_keeps_animating_without_a_ui_publication() {
        let mut uploads = ViewportUploads::default();
        let mut phases = Vec::new();
        let glint = UiGlintSettings {
            strength: 1.0,
            speed: 0.5,
        };
        uploads.upload(
            [800, 600],
            glint,
            false,
            || panic!("unused clock"),
            |value| phases.push(value.time_seconds),
        );
        for elapsed in [40.0, 42.0, 42.0] {
            uploads.upload(
                [800, 600],
                glint,
                true,
                || elapsed,
                |value| phases.push(value.time_seconds),
            );
        }
        uploads.upload(
            [800, 600],
            glint,
            false,
            || panic!("unused clock"),
            |value| phases.push(value.time_seconds),
        );
        uploads.upload(
            [800, 600],
            glint,
            true,
            || 60.0,
            |value| phases.push(value.time_seconds),
        );
        assert_eq!(phases, [0.0, 20.0, 21.0, 0.0, 30.0]);
    }

    /// Disabled or stationary glint does not consume a clock or upload unchanged bytes.
    #[test]
    fn disabled_and_stationary_glint_skip_clock_work() {
        for glint in [
            UiGlintSettings {
                strength: 0.0,
                speed: 1.0,
            },
            UiGlintSettings {
                strength: 1.0,
                speed: 0.0,
            },
        ] {
            let mut uploads = ViewportUploads::default();
            for _ in 0..10 {
                uploads.upload(
                    [800, 600],
                    glint,
                    true,
                    || panic!("inactive glint sampled the clock"),
                    |value| assert_eq!(value.time_seconds, 0.0),
                );
            }
            assert_eq!(uploads.writes, 1);
        }
    }
}
