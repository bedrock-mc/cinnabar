use bevy::{prelude::Time, time::Real};
use std::time::Duration;
use web_time::Instant;

/// Maps a queued tick's elapsed sampling time to the application clock.
pub(super) fn sample_instant(time: &Time<Real>, sampled_at: Option<Duration>) -> Option<Instant> {
    time.first_update()?.checked_add(sampled_at?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn movement_sample_time_uses_its_creation_frame_after_later_updates() {
        let startup = Instant::now();
        let mut time = Time::<Real>::new(startup);
        let first = startup + Duration::from_secs(2);
        time.update_with_instant(first);
        let sampled_at = Duration::from_millis(50);
        time.update_with_instant(first + sampled_at);
        time.update_with_instant(first + Duration::from_secs(1));
        assert_eq!(
            sample_instant(&time, Some(sampled_at)),
            Some(first + sampled_at)
        );
        assert_eq!(sample_instant(&time, None), None);
        assert_eq!(
            sample_instant(&Time::<Real>::new(startup), Some(sampled_at)),
            None
        );
    }
}
