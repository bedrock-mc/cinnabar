use super::DrawingFrame;
use bevy::{prelude::*, render::extract_resource::ExtractResource};

/// Real-time animation clock, reset by the trusted connection fence rather than pause state.
#[derive(Resource, Clone, Default, ExtractResource)]
#[extract_app(bevy::render::RenderApp)]
pub(super) struct AnimationClock {
    connection: u64,
    start: f64,
    pub phase: f32,
}
impl AnimationClock {
    /// Samples explicit real elapsed seconds; a disconnected or new session starts at zero.
    fn sample(&mut self, connection: u64, now: f64) {
        if connection != self.connection || connection == 0 {
            self.start = now;
            self.connection = connection;
        }
        self.phase = mod_render::material::phase(now - self.start);
    }
}
/// Reads the application clock only at the rendering edge, after session publication.
pub(super) fn advance(
    time: Res<Time<Real>>,
    frame: Res<DrawingFrame>,
    mut clock: ResMut<AnimationClock>,
) {
    clock.sample(frame.connection, time.elapsed_secs_f64());
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_time_wraps_and_new_sessions_restart() {
        let mut clock = AnimationClock::default();
        clock.sample(1, 100.);
        assert_eq!(clock.phase, 0.);
        clock.sample(1, 100. + mod_api::materials::ANIMATION_WRAP_SECONDS * 1.25);
        assert_eq!(clock.phase, 0.25);
        clock.sample(2, 6000.);
        assert_eq!(clock.phase, 0.);
        clock.sample(0, 7000.);
        assert_eq!(clock.phase, 0.);
    }
}
