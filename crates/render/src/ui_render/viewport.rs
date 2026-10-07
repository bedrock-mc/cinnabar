//! Retains the exact viewport bytes already written to the GPU.

use super::shader::UiViewportUniform;

#[derive(Default)]
pub(super) struct ViewportUniformCache(Option<UiViewportUniform>);

impl ViewportUniformCache {
    /// Returns changed GPU bytes, keeping unused animation time fixed when no glint draws.
    pub(super) fn update(
        &mut self,
        mut viewport: UiViewportUniform,
        animated: bool,
    ) -> Option<UiViewportUniform> {
        if !animated {
            viewport.time_seconds = 0.0;
        }
        if self
            .0
            .as_ref()
            .is_some_and(|held| bytemuck::bytes_of(held) == bytemuck::bytes_of(&viewport))
        {
            return None;
        }
        self.0 = Some(viewport);
        Some(viewport)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a valid uniform whose time may change without affecting static UI pixels.
    fn uniform(time_seconds: f32) -> UiViewportUniform {
        UiViewportUniform {
            viewport_size: [1920.0, 1080.0],
            time_seconds,
            glint_strength: super::super::UiGlintSettings::default().strength,
        }
    }

    #[test]
    fn static_ui_writes_once_despite_elapsed_time() {
        let mut cache = ViewportUniformCache::default();
        let first = cache.update(uniform(1.0), false).unwrap();
        assert_eq!(first.time_seconds, 0.0);
        let warm_writes = [1.0, 2.0, 30.0, 3600.0]
            .into_iter()
            .filter(|time| cache.update(uniform(*time), false).is_some())
            .count();
        assert_eq!(warm_writes, 0);
    }

    #[test]
    fn resize_and_glint_strength_write_once_per_change() {
        let mut cache = ViewportUniformCache::default();
        assert!(cache.update(uniform(1.0), false).is_some());
        let mut viewport = uniform(2.0);
        viewport.viewport_size[0] *= 2.0;
        assert!(cache.update(viewport, false).is_some());
        assert!(cache.update(viewport, false).is_none());
        viewport.glint_strength *= 0.5;
        assert!(cache.update(viewport, false).is_some());
        assert!(cache.update(viewport, false).is_none());
    }

    #[test]
    fn glint_animates_and_stopping_it_resets_time_once() {
        let mut cache = ViewportUniformCache::default();
        assert!(cache.update(uniform(1.0), false).is_some());
        for time in [1.0, 2.0, 3.0] {
            let written = cache.update(uniform(time), true).unwrap();
            assert_eq!(written.time_seconds, time);
            assert!(cache.update(uniform(time), true).is_none());
        }
        assert_eq!(cache.update(uniform(4.0), false).unwrap().time_seconds, 0.0);
        assert!(cache.update(uniform(5.0), false).is_none());
        assert_eq!(cache.update(uniform(6.0), true).unwrap().time_seconds, 6.0);
    }

    #[test]
    fn gpu_bytes_retain_float_bit_patterns() {
        let mut cache = ViewportUniformCache::default();
        let mut viewport = uniform(0.0);
        viewport.glint_strength = f32::from_bits(0x7fc0_0001);
        assert!(cache.update(viewport, false).is_some());
        assert!(cache.update(viewport, false).is_none());
        viewport.glint_strength = -0.0;
        assert!(cache.update(viewport, false).is_some());
        viewport.glint_strength = 0.0;
        assert!(cache.update(viewport, false).is_some());
        assert!(cache.update(viewport, false).is_none());
    }
}
