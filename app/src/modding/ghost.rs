//! Own last-forwarded position, smoothed for display without predicting beyond the sample.

use super::packet_delay::RealPositionSnapshot;
use bevy::prelude::*;
use client_ui::ui_runtime::UiRuntime;
use render::ModRenderScene;
use {
    crate::{app::ClientFrameSet, menu::MenuRuntime, runtime::world::ClientWorld},
    client_presentation::camera::FlyCamera,
};

pub(super) fn configure(app: &mut App) {
    app.add_systems(
        Update,
        publish
            .after(super::packet_delay::publish_packet_delay)
            .after(ClientFrameSet::NetworkSend)
            .before(ClientFrameSet::UiPreparation),
    );
}

#[derive(Default)]
pub(crate) struct SmoothedPosition {
    session: u64,
    position: Option<Vec3>,
}

impl SmoothedPosition {
    fn update(&mut self, snapshot: Option<&RealPositionSnapshot>, seconds: f32) -> Option<Vec3> {
        let target = snapshot
            .filter(|sample| sample.session_id != 0)
            .and_then(|sample| sample.position)
            .map(Vec3::from_array)
            .filter(|point| point.is_finite());
        let Some(target) = target else {
            *self = Self::default();
            return None;
        };
        let session = snapshot.unwrap().session_id;
        let position = match self.position {
            Some(previous)
                if self.session == session && previous.distance_squared(target) < 64.0 =>
            {
                // Exponential convergence has the same response at every frame rate.
                let weight = 1.0 - (-40.0 * seconds.max(0.0)).exp();
                let point = previous.lerp(target, weight);
                if point.distance_squared(target) < 1e-8 {
                    target
                } else {
                    point
                }
            }
            _ => target,
        };
        self.session = session;
        self.position = Some(position);
        Some(position)
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn publish(
    snapshot: Option<Res<RealPositionSnapshot>>,
    world: Option<Res<ClientWorld>>,
    menu: Option<Res<MenuRuntime>>,
    ui: Option<Res<UiRuntime>>,
    player: Option<Res<crate::player_runtime::PlayerRuntime>>,
    time: Option<Res<Time>>,
    cameras: Query<&Transform, With<FlyCamera>>,
    mut smooth: Local<SmoothedPosition>,
    mut scene: ResMut<ModRenderScene>,
) {
    let visible = !world
        .as_deref()
        .is_none_or(|world| world.stream.is_none() || world.fatal_error.is_some())
        && !menu.as_deref().is_some_and(MenuRuntime::is_visible)
        && ui
            .as_deref()
            .zip(player.as_deref())
            .is_some_and(|(ui, player)| !ui.ui_focused(player));
    let feet = smooth.update(
        visible.then_some(snapshot.as_deref()).flatten(),
        time.as_deref().map_or(0.0, Time::delta_secs),
    );
    let bounds = feet.and_then(|feet| {
        let aabb =
            sim::Aabb::player_at(sim::Vec3::new(feet.x as f64, feet.y as f64, feet.z as f64));
        let point = |v: sim::Vec3| [v.x as f32, v.y as f32, v.z as f32];
        let bounds = [point(aabb.min), point(aabb.max)];
        let camera = cameras.single().ok()?.translation;
        // A first-person camera inside its own box must not tint the entire viewport.
        if (0..3).all(|axis| camera[axis] >= bounds[0][axis] && camera[axis] <= bounds[1][axis]) {
            return None;
        }
        Some(bounds)
    });
    scene.set_position_box(bounds);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(session_id: u64, x: f32) -> RealPositionSnapshot {
        RealPositionSnapshot {
            session_id,
            position: Some([x, 0.0, 0.0]),
        }
    }

    #[test]
    fn smoothing_is_frame_rate_independent_and_never_overshoots() {
        let mut slow = SmoothedPosition::default();
        let mut fast = SmoothedPosition::default();
        for state in [&mut slow, &mut fast] {
            state.update(Some(&sample(1, 0.0)), 0.0);
        }
        let target = sample(1, 1.0);
        for _ in 0..3 {
            slow.update(Some(&target), 1.0 / 30.0);
        }
        for _ in 0..12 {
            fast.update(Some(&target), 1.0 / 120.0);
        }
        assert!(slow.position.unwrap().distance(fast.position.unwrap()) < 1e-6);
        assert!((0.0..1.0).contains(&slow.position.unwrap().x));
        slow.update(Some(&target), 1.0);
        assert_eq!(slow.position, Some(Vec3::X));
    }

    #[test]
    fn new_sessions_teleports_and_reenabled_tracking_snap() {
        let mut smooth = SmoothedPosition::default();
        smooth.update(Some(&sample(1, 0.0)), 0.0);
        assert_eq!(smooth.update(Some(&sample(2, 1.0)), 0.0), Some(Vec3::X));
        assert_eq!(
            smooth.update(Some(&sample(2, 20.0)), 0.0),
            Some(Vec3::new(20.0, 0.0, 0.0))
        );
        assert_eq!(smooth.update(None, 0.0), None);
        assert_eq!(smooth.update(Some(&sample(2, 1.0)), 0.0), Some(Vec3::X));
        assert_eq!(smooth.update(Some(&sample(2, f32::NAN)), 0.0), None);
    }

    #[test]
    fn world_loss_clears_previously_published_geometry() {
        let mut scene = ModRenderScene::default();
        scene.set_position_box(Some([[0.0; 3], [1.0; 3]]));
        assert!(scene.vertex_count() > 0);
        let mut app = App::new();
        app.insert_resource(scene).add_systems(Update, publish);
        app.update();
        assert_eq!(app.world().resource::<ModRenderScene>().vertex_count(), 0);
    }
}
