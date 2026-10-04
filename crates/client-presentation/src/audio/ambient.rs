//! Timers and selection for music, cave/nether mood and additions sounds.

/// Music scheduling: waits a definition-driven random delay, then asks for a track.
#[derive(Debug, Default)]
pub(super) struct MusicScheduler {
    key: Option<Box<str>>,
    remaining: f32,
    was_playing: bool,
}

impl MusicScheduler {
    /// True when a track should start now; `delay` is `(min, max)` seconds for `key`.
    pub(super) fn update(
        &mut self,
        key: &str,
        delay: (f32, f32),
        playing: bool,
        dt: f32,
        mut unit: impl FnMut() -> f32,
    ) -> bool {
        let roll = |unit: &mut dyn FnMut() -> f32| delay.0 + (delay.1 - delay.0).max(0.0) * unit();
        if self.key.as_deref() != Some(key) {
            self.key = Some(key.into());
            self.remaining = roll(&mut unit);
        }
        if playing {
            self.was_playing = true;
            return false;
        }
        if self.was_playing {
            self.was_playing = false;
            self.remaining = roll(&mut unit);
        }
        self.remaining -= dt;
        if self.remaining <= 0.0 {
            self.remaining = roll(&mut unit).max(1.0);
            return true;
        }
        false
    }
}

/// Random-interval one-shots (cave mood, nether additions); paused while `active` is false.
#[derive(Debug)]
pub(super) struct IntervalTimer {
    remaining: f32,
    range: (f32, f32),
}

impl IntervalTimer {
    pub(super) fn new(range: (f32, f32)) -> Self {
        Self {
            remaining: range.1,
            range,
        }
    }

    pub(super) fn tick(&mut self, active: bool, dt: f32, unit: f32) -> bool {
        if !active {
            return false;
        }
        self.remaining -= dt;
        if self.remaining > 0.0 {
            return false;
        }
        self.remaining = self.range.0 + (self.range.1 - self.range.0) * unit;
        true
    }
}

/// Cave mood interval in seconds; needs native measurement.
pub(super) const MOOD_INTERVAL: (f32, f32) = (40.0, 120.0);
/// Nether additions interval in seconds; needs native measurement.
pub(super) const ADDITIONS_INTERVAL: (f32, f32) = (15.0, 45.0);

/// Ambience definition prefix for a dimension id, `None` where no ambience loop applies.
pub(super) fn dimension_ambience(dimension: i32) -> Option<&'static str> {
    match dimension {
        1 => Some("ambient.nether_wastes"),
        _ => None,
    }
}

/// Music key of `music_definitions.json` for the current context.
pub(super) fn music_key(in_world: bool, dimension: i32, creative: bool) -> &'static str {
    match (in_world, dimension, creative) {
        (false, _, _) => "menu",
        (true, 1, _) => "nether",
        (true, 2, _) => "end",
        (true, _, true) => "creative",
        _ => "game",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn music_waits_the_delay_then_reschedules_after_a_track_ends() {
        let mut scheduler = MusicScheduler::default();
        assert!(!scheduler.update("game", (10.0, 10.0), false, 5.0, || 0.0));
        assert!(scheduler.update("game", (10.0, 10.0), false, 5.0, || 0.0));
        assert!(!scheduler.update("game", (10.0, 10.0), true, 30.0, || 0.0));
        assert!(!scheduler.update("game", (10.0, 10.0), false, 9.0, || 0.0));
        assert!(scheduler.update("game", (10.0, 10.0), false, 2.0, || 0.0));
    }

    #[test]
    fn zero_delay_menu_music_starts_immediately_and_key_changes_reschedule() {
        let mut scheduler = MusicScheduler::default();
        assert!(scheduler.update("menu", (0.0, 0.0), false, 0.016, || 0.0));
        assert!(!scheduler.update("game", (60.0, 60.0), false, 0.016, || 0.0));
    }

    #[test]
    fn interval_timer_pauses_while_inactive() {
        let mut timer = IntervalTimer::new((10.0, 10.0));
        assert!(!timer.tick(false, 100.0, 0.0));
        assert!(!timer.tick(true, 9.0, 0.0));
        assert!(timer.tick(true, 2.0, 0.0));
        assert!(!timer.tick(true, 5.0, 0.0));
    }

    #[test]
    fn keys_follow_context() {
        assert_eq!(music_key(false, 0, false), "menu");
        assert_eq!(music_key(true, 1, true), "nether");
        assert_eq!(music_key(true, 0, true), "creative");
        assert_eq!(music_key(true, 0, false), "game");
    }
}
