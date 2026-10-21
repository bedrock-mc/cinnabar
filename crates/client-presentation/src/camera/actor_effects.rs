//! Actor-clock damage rotation composed onto the rendered player camera.

use bevy::prelude::{Mat4, Quat, Query, Res, ResMut, SystemSet, Transform, With};

use super::{CameraSettingsAuthority, FirstPersonHandMotion, FlyCamera, ServerCameraView};
use crate::observations::WorldObservation;
use semantic_input::PerspectiveMode;

/// Completes actor camera effects before an explicit rendered-pose override.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ActorCameraEffects;

/// Samples hurt and bob on their own clocks, applying movement bob to the selected views.
#[allow(clippy::too_many_arguments)]
pub fn apply_actor_damage_camera_rotation(
    settings: Res<CameraSettingsAuthority>,
    mut hand: ResMut<FirstPersonHandMotion>,
    legacy: Option<Res<super::legacy::LegacyCameraMotion>>,
    server: Res<ServerCameraView>,
    world: Option<WorldObservation<'_>>,
    partial_tick: f32,
    physics_partial_tick: f32,
    mut cameras: Query<&mut Transform, With<FlyCamera>>,
) {
    hand.legacy_effect = None;
    if !server.renders_first_person(settings.perspective() == PerspectiveMode::FirstPerson) {
        return;
    }
    let alpha = render_model::legacy_combat::partial_tick(partial_tick);
    if render_api::DEFAULT_COMBAT_VISUALS == render_api::CombatVisuals::Legacy112 {
        let stream = world.and_then(|world| world.stream);
        let actor =
            stream.and_then(|stream| stream.authority().actor(stream.local_player_runtime_id()));
        let geometry = stream
            .and_then(|stream| {
                stream
                    .authority()
                    .actor_rig(stream.local_player_runtime_id())
            })
            .and_then(|rig| rig.skin_geometry)
            .map(|geometry| geometry.as_ref());
        let endpoints = legacy
            .as_deref()
            .map(|legacy| legacy.state.endpoints)
            .unwrap_or_default();
        let sample = |bobbing| {
            sampled_legacy_effect(
                actor,
                endpoints,
                alpha,
                physics_partial_tick,
                bobbing,
                settings.hurt_camera_style(),
                geometry,
            )
        };
        let mode = settings.feel().view_bobbing;
        let effect = sample(mode.hand_enabled());
        hand.legacy_effect = Some(effect);
        let camera_effect = if mode.hand_enabled() == mode.camera_enabled() {
            effect
        } else {
            sample(false)
        };
        if camera_effect.is_finite() {
            for mut camera in &mut cameras {
                // Inverting the selected view effect gives Hurt × Bob × Vbase.
                *camera = Transform::from_matrix(camera.to_matrix() * camera_effect.inverse());
            }
        }
        return;
    }
    let damage = world
        .and_then(|world| world.stream)
        .and_then(|stream| stream.authority().actor(stream.local_player_runtime_id()))
        .map_or(Quat::IDENTITY, |actor| {
            let death_ticks = actor
                .attributes
                .get("minecraft:health")
                .filter(|health| health.current.ceil() < 1.0)
                .map(|_| f32::from(actor.status.native_death_ticks()) + alpha);
            let hurt_progress = if actor.status.hurt_time > 0 {
                (f32::from(actor.status.hurt_time) - alpha)
                    / f32::from(client_world::HURT_DURATION_TICKS)
            } else {
                0.0
            };
            damage_rotation(
                death_ticks,
                hurt_progress,
                if settings.hurt_camera_style().old_tilt() {
                    0.0
                } else {
                    actor.status.hurt_direction.unwrap_or(0.0).to_radians()
                },
                settings.hurt_camera_style().intensity(),
            )
        });
    let bob = if settings.feel().view_bobbing.camera_enabled() {
        hand.bob.matrix().inverse()
    } else {
        Mat4::IDENTITY
    };
    let effect = Mat4::from_quat(damage) * bob;
    if effect == Mat4::IDENTITY || !effect.is_finite() {
        return;
    }
    for mut camera in &mut cameras {
        *camera = Transform::from_matrix(camera.to_matrix() * effect);
    }
}

/// Shares actor-clock hurt and physics-clock bob between world and first-person draws.
fn sampled_legacy_effect(
    actor: Option<&client_world::ActorSnapshot>,
    endpoints: render_model::legacy_combat::LegacyCameraEndpoints,
    actor_alpha: f32,
    physics_alpha: f32,
    view_bobbing: bool,
    style: render_api::HurtCameraStyle,
    geometry: Option<&assets::SkinGeometry>,
) -> Mat4 {
    let Some(actor) = actor else {
        return Mat4::IDENTITY;
    };
    let death_ticks = (actor.status.dead
        || actor
            .attributes
            .get("minecraft:health")
            .is_some_and(|health| health.current.ceil() < 1.0))
    .then(|| f32::from(actor.status.native_death_ticks()));
    let hurt = render_model::legacy_combat::legacy_hurt_matrix(
        actor.status.hurt_time,
        if style.old_tilt() {
            0.0
        } else {
            actor.status.hurt_direction.unwrap_or(0.0)
        },
        death_ticks,
        actor_alpha,
        style.intensity(),
    );
    let bob = if view_bobbing
        && matches!(actor.kind, protocol::ActorKind::Player { .. })
        && geometry.is_none_or(|geometry| !geometry.animation_no_head_bob)
    {
        endpoints.bob_matrix(physics_alpha)
    } else {
        Mat4::IDENTITY
    };
    hurt * bob
}

/// Returns native camera-local damage rotation from sampled actor state.
fn damage_rotation(
    death_ticks: Option<f32>,
    hurt_progress: f32,
    hurt_angle: f32,
    strength: f32,
) -> Quat {
    let mut rotation = Quat::IDENTITY;
    if let Some(ticks) = death_ticks.filter(|ticks| *ticks > 0.0) {
        rotation = Quat::from_rotation_z(-super::hurt::death_roll_degrees(ticks).to_radians());
    }
    if hurt_progress > 0.0 {
        let phase =
            hurt_progress * hurt_progress * hurt_progress * hurt_progress * std::f32::consts::PI;
        let shake = sim::minecraft_sin(f64::from(phase)) as f32;
        let angle = (shake * super::hurt::HURT_TILT_DEGREES * strength).to_radians();
        rotation *= Quat::from_rotation_y(hurt_angle)
            * Quat::from_rotation_z(angle)
            * Quat::from_rotation_y(-hurt_angle);
    }
    rotation
}

/// Samples death time after actor ticking and clears it when the local actor leaves or recovers.
pub fn collect_death_fov_inputs(
    stream: Option<&chunk_pipeline::WorldStream>,
    partial_tick: Option<bevy::prelude::Res<crate::actor_publication::ActorFramePartialTick>>,
    mut inputs: bevy::prelude::ResMut<crate::camera::CameraFovInputs>,
) {
    let ticks = stream
        .and_then(|stream| stream.authority().actor(stream.local_player_runtime_id()))
        .filter(|actor| actor.status.dead)
        .map(|actor| actor.status.native_death_ticks());
    inputs.set_death_ticks(ticks, partial_tick.map_or(0.0, |tick| tick.0));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The old tilt ignores the supplied damage direction; disabling shake preserves ordinary bob.
    #[test]
    fn hurt_camera_style_controls_direction_and_shake_without_changing_bob() {
        let mut stream = crate::presentation::combat::tests::player_stream();
        let id = stream.local_player_runtime_id();
        crate::presentation::combat::tests::event(&mut stream, 2, protocol::ActorStatusKind::Hurt);
        let mut actor = stream.authority().actor(id).unwrap().clone();
        let old = render_api::HurtCameraStyle::new(1.0, true).unwrap();
        let sample = |actor: &client_world::ActorSnapshot, style| {
            sampled_legacy_effect(
                Some(actor),
                Default::default(),
                0.5,
                0.5,
                false,
                style,
                None,
            )
        };
        actor.status.hurt_direction = Some(0.0);
        let fixed = sample(&actor, old);
        actor.status.hurt_direction = Some(90.0);
        assert_eq!(sample(&actor, old), fixed);
        assert_ne!(
            sample(
                &actor,
                render_api::HurtCameraStyle::new(1.0, false).unwrap()
            ),
            fixed
        );
        let endpoints = render_model::legacy_combat::LegacyCameraEndpoints {
            walked: [0.3, 0.6],
            bob: [0.05, 0.1],
            fall_degrees: [-4.0, -2.0],
        };
        let disabled = render_api::HurtCameraStyle::new(0.0, true).unwrap();
        assert_eq!(sample(&actor, disabled), Mat4::IDENTITY);
        assert_eq!(
            sampled_legacy_effect(Some(&actor), endpoints, 0.5, 0.5, true, disabled, None),
            endpoints.bob_matrix(0.5)
        );
    }
    use bevy::prelude::Vec3;

    #[test]
    fn actor_half_hurt_rolls_positive_z_and_direction_rotates_its_axis() {
        let right = damage_rotation(None, 0.5, 0.0, 1.0) * Vec3::X;
        assert!(right.y > 0.047 && right.y < 0.048);
        assert!(right.z.abs() < 1e-6);
        let up = damage_rotation(None, 0.5, std::f32::consts::FRAC_PI_2, 1.0) * Vec3::Y;
        assert!(up.z > 0.047 && up.z < 0.048);
        assert!(up.x.abs() < 1e-6);
    }

    #[test]
    fn native_death_counter_continues_beyond_the_body_clock() {
        let right = damage_rotation(Some(200.0), 0.0, 0.0, 1.0) * Vec3::X;
        assert!((right.x - 20.0_f32.to_radians().cos()).abs() < 1e-6);
        assert!((right.y + 20.0_f32.to_radians().sin()).abs() < 1e-6);
        assert_eq!(damage_rotation(Some(-1.0), 0.0, 0.0, 1.0), Quat::IDENTITY);
    }

    #[test]
    fn damage_bob_scale_does_not_disable_the_independent_death_rotation() {
        assert_eq!(damage_rotation(None, 0.5, 0.0, 0.0), Quat::IDENTITY);
        assert_eq!(
            damage_rotation(Some(200.0), 0.5, 0.0, 0.0),
            damage_rotation(Some(200.0), 0.0, 0.0, 1.0)
        );
    }
}

#[cfg(test)]
mod legacy_effect_tests {
    use super::*;

    #[test]
    fn authored_bob_suppression_keeps_hurt_rotation() {
        let mut geometry = serde_json::json!({"description":{"identifier":"geometry.body"},
            "bones":[{"name":"body"}]});
        geometry[assets::entity::GEOMETRY_NO_HEAD_BOB_FIELD] = true.into();
        let source =
            serde_json::json!({"format_version":"1.12.0", "minecraft:geometry":[geometry]});
        let skin = assets::parse_skin_geometry(
            r#"{"geometry":{"default":"geometry.body"}}"#,
            &source.to_string(),
        )
        .unwrap()
        .unwrap();
        let mut stream = crate::presentation::combat::tests::player_stream();
        let id = stream.local_player_runtime_id();
        crate::presentation::combat::tests::event(&mut stream, 2, protocol::ActorStatusKind::Hurt);
        let actor = stream.authority().actor(id);
        let endpoints = render_model::legacy_combat::LegacyCameraEndpoints {
            walked: [0.3, 0.6],
            bob: [0.05, 0.1],
            fall_degrees: [-4.0, -2.0],
        };
        let suppressed = sampled_legacy_effect(
            actor,
            endpoints,
            0.5,
            0.5,
            true,
            render_api::HurtCameraStyle::new(1.0, false).unwrap(),
            Some(&skin),
        );
        let hurt_only = sampled_legacy_effect(
            actor,
            Default::default(),
            0.5,
            0.5,
            true,
            render_api::HurtCameraStyle::new(1.0, false).unwrap(),
            None,
        );
        assert_eq!(suppressed, hurt_only);
        assert_ne!(suppressed, Mat4::IDENTITY);
        assert_ne!(
            suppressed,
            sampled_legacy_effect(
                actor,
                endpoints,
                0.5,
                0.5,
                true,
                render_api::HurtCameraStyle::new(1.0, false).unwrap(),
                None
            )
        );
    }

    #[test]
    fn hurt_and_death_keep_the_actor_fraction_when_physics_has_a_different_fraction() {
        let mut stream = crate::presentation::combat::tests::player_stream();
        let id = stream.local_player_runtime_id();
        crate::presentation::combat::tests::event(&mut stream, 2, protocol::ActorStatusKind::Hurt);
        let endpoints = render_model::legacy_combat::LegacyCameraEndpoints {
            walked: [0.3, 0.6],
            bob: [0.05, 0.1],
            fall_degrees: [-4.0, -2.0],
        };
        for dead in [false, true] {
            if dead {
                crate::presentation::combat::tests::event(
                    &mut stream,
                    3,
                    protocol::ActorStatusKind::Death,
                );
            }
            let actor = stream.authority().actor(id).unwrap();
            let expected = render_model::legacy_combat::legacy_hurt_matrix(
                actor.status.hurt_time,
                actor.status.hurt_direction.unwrap_or(0.0),
                dead.then(|| f32::from(actor.status.native_death_ticks())),
                0.85,
                1.0,
            ) * endpoints.bob_matrix(0.2);
            let actual = sampled_legacy_effect(
                Some(actor),
                endpoints,
                0.85,
                0.2,
                true,
                render_api::HurtCameraStyle::new(1.0, false).unwrap(),
                None,
            );
            assert_eq!(actual, expected);
            assert!(!actual.abs_diff_eq(
                sampled_legacy_effect(
                    Some(actor),
                    endpoints,
                    0.2,
                    0.2,
                    true,
                    render_api::HurtCameraStyle::new(1.0, false).unwrap(),
                    None
                ),
                1e-5,
            ));
        }
    }
}
