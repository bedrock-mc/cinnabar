//! Per-frame camera presentation: FOV inputs, view bob, hurt tilt, server camera, shake and overlays.
//! Everything here composes onto the rendered camera only; look, movement and interaction origins are untouched.

use crate::local_player::LocalViewPose;
use crate::server_camera::ServerCameraInstructions;

use bevy::prelude::{
    EulerRot, Mat4, Quat, Query, Res, ResMut, Resource, Time, Transform, Vec3, With,
};
use semantic_input::{Action, PerspectiveMode};

use super::{
    CameraSettingsAuthority, FlyCamera,
    bob::{HandSwayState, ViewEffect, WalkBobState, walk_bob_effect},
    fov::CameraFovInputs,
    hurt::CameraHurtState,
    overlay::{
        HeadMedium, PortalProgress, ScreenEffectInputs, ScreenOverlays, VisionEffects,
        compute_overlays, nausea_roll_radians, probe_head_medium,
    },
    server_view::{ActorView, ServerCameraView, ViewContext},
};

const EFFECT_ID_SPEED: i32 = 1;
const EFFECT_ID_SLOWNESS: i32 = 2;
const EFFECT_ID_NAUSEA: i32 = 9;
const EFFECT_ID_BLINDNESS: i32 = 15;
const EFFECT_ID_NIGHT_VISION: i32 = 16;
const EFFECT_ID_DARKNESS: i32 = 30;
const CARVED_PUMPKIN_IDENTIFIER: &str = "minecraft:carved_pumpkin";
const VISION_FADE_SECONDS: f32 = 1.0;

/// Facts other lanes own and publish each frame; both default to false.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ScreenEffectFacts {
    pub on_fire: bool,
    pub in_portal: bool,
}

/// First-person hand motion for the equipment lane, all in view space.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct FirstPersonHandMotion {
    pub bob: ViewEffect,
    pub hurt: Mat4,
    pub sway_pitch_radians: f32,
    pub sway_yaw_radians: f32,
}

impl Default for FirstPersonHandMotion {
    fn default() -> Self {
        Self {
            bob: ViewEffect::NONE,
            hurt: Mat4::IDENTITY,
            sway_pitch_radians: 0.0,
            sway_yaw_radians: 0.0,
        }
    }
}

pub fn collect_fov_inputs(
    input: crate::observations::InputObservation<'_>,
    settings: Res<CameraSettingsAuthority>,
    ui: Option<&client_ui::ui_runtime::UiRuntime>,
    physics: Option<&dyn crate::observations::PhysicsObservation>,
    mut inputs: ResMut<CameraFovInputs>,
) {
    inputs.sprinting = physics
        .and_then(|physics| physics.latest_sneak_sprint())
        .map_or_else(
            || input.phase(Action::Sprint).held && input.movement()[1] > 0.0,
            |(_, sprinting)| sprinting,
        );
    inputs.fov_effects_scale = settings.feel().fov_effects_scale;
    let (mut speed, mut slowness) = (0, 0);
    if let Some(ui) = ui {
        for effect in ui.gameplay_hud().effects() {
            let levels = u32::try_from(effect.amplifier.saturating_add(1)).unwrap_or(0);
            match effect.effect_id {
                EFFECT_ID_SPEED => speed = speed.max(levels),
                EFFECT_ID_SLOWNESS => slowness = slowness.max(levels),
                _ => {}
            }
        }
    }
    inputs.speed_levels = speed;
    inputs.slowness_levels = slowness;
}

#[allow(clippy::too_many_arguments)]
pub fn advance_presentation_state(
    time: Res<Time>,
    settings: Res<CameraSettingsAuthority>,
    view: Res<LocalViewPose>,
    physics: Option<&dyn crate::observations::PhysicsObservation>,
    mut bob: ResMut<WalkBobState>,
    mut sway: ResMut<HandSwayState>,
    mut hurt: ResMut<CameraHurtState>,
    mut hand: ResMut<FirstPersonHandMotion>,
) {
    let dt = time.delta_secs();
    let on_ground = physics
        .and_then(|physics| physics.state())
        .is_some_and(|state| state.on_ground);
    bob.advance(view.eye_translation(), on_ground, true, dt);
    hurt.advance(dt);
    let (yaw, pitch, _) = view.rotation().to_euler(EulerRot::YXZ);
    sway.advance(pitch, yaw, dt);
    hand.bob = if settings.feel().view_bobbing {
        walk_bob_effect(bob.walk_distance(), bob.bob())
    } else {
        ViewEffect::NONE
    };
    hand.hurt = Mat4::from_quat(Quat::IDENTITY.slerp(
        Quat::from_mat4(&hurt.view_matrix(yaw)),
        settings.feel().damage_bob,
    ));
    // Vanilla sways the hand only while view bobbing is on.
    let (sway_pitch, sway_yaw) = if settings.feel().view_bobbing {
        sway.sway_radians()
    } else {
        (0.0, 0.0)
    };
    hand.sway_pitch_radians = sway_pitch;
    hand.sway_yaw_radians = sway_yaw;
}

fn approach(current: f32, target: f32, step: f32) -> f32 {
    if target > current {
        (current + step).min(target)
    } else {
        (current - step).max(target)
    }
}

#[allow(clippy::too_many_arguments)]
pub fn update_screen_overlays(
    player_runtime: &player_state::PlayerState,
    time: Res<Time>,
    settings: Res<CameraSettingsAuthority>,
    view: Res<LocalViewPose>,
    facts: Res<ScreenEffectFacts>,
    fov_inputs: Res<CameraFovInputs>,
    server: Res<ServerCameraView>,
    ui: Option<&client_ui::ui_runtime::UiRuntime>,
    client_world: Option<crate::observations::WorldObservation<'_>>,
    collisions: Option<&dyn crate::observations::CollisionLookup>,
    mut portal: ResMut<PortalProgress>,
    mut medium: ResMut<HeadMedium>,
    mut vision: ResMut<VisionEffects>,
    mut overlays: ResMut<ScreenOverlays>,
) {
    let dt = time.delta_secs();
    portal.advance(facts.in_portal, dt);
    let stream = client_world
        .as_ref()
        .and_then(|world| world.stream.as_ref());
    *medium = match (stream, collisions) {
        (Some(stream), Some(collisions)) => {
            let world = sim::PaletteWorld::new(
                stream.collision_store(),
                collisions.registry(stream.network_id_mode()),
                stream.current_dimension(),
            );
            probe_head_medium(&world, view.eye_translation())
        }
        _ => HeadMedium::Air,
    };

    let mut active = [false; 4];
    let mut freezing = 0.0;
    let mut pumpkin = false;
    if let Some(ui) = ui {
        let hud = ui.gameplay_hud();
        for effect in hud.effects() {
            match effect.effect_id {
                EFFECT_ID_NAUSEA => active[0] = true,
                EFFECT_ID_BLINDNESS => active[1] = true,
                EFFECT_ID_NIGHT_VISION => active[2] = true,
                EFFECT_ID_DARKNESS => active[3] = true,
                _ => {}
            }
        }
        freezing = hud.freezing_strength();
        let helmet = ui.local_armor(player_runtime).helmet;
        pumpkin = stream
            .filter(|_| !helmet.is_empty())
            .and_then(|stream| stream.authority().canonical_item_stack(&helmet)?.identifier)
            .is_some_and(|identifier| &*identifier == CARVED_PUMPKIN_IDENTIFIER);
    }
    let step = if dt.is_finite() && dt > 0.0 {
        dt / VISION_FADE_SECONDS
    } else {
        0.0
    };
    let goal = |on: bool| if on { 1.0_f32 } else { 0.0 };
    vision.nausea = approach(vision.nausea, goal(active[0]).max(portal.value()), step);
    vision.blindness = approach(vision.blindness, goal(active[1]), step);
    vision.night_vision = approach(vision.night_vision, goal(active[2]), step);
    vision.darkness = approach(vision.darkness, goal(active[3]), step);

    overlays.layers = compute_overlays(&ScreenEffectInputs {
        first_person: settings.perspective() == PerspectiveMode::FirstPerson,
        head: *medium,
        carved_pumpkin_worn: pumpkin,
        on_fire: facts.on_fire,
        spyglass_scoping: fov_inputs.spyglass_scoping,
        freezing_strength: freezing,
        portal_progress: portal.value(),
        server_fade: server.fade_overlay(),
        distortion_scale: settings.feel().distortion_scale,
    });
}

/// Composes hurt tilt, walk bob, nausea wobble, server pose and shake onto the camera transform.
#[allow(clippy::too_many_arguments)]
pub fn apply_camera_presentation(
    time: Res<Time>,
    settings: Res<CameraSettingsAuthority>,
    instructions: Option<Res<ServerCameraInstructions>>,
    hand: Res<FirstPersonHandMotion>,
    vision: Res<VisionEffects>,
    view: Res<LocalViewPose>,
    client_world: Option<crate::observations::WorldObservation<'_>>,
    mut server: ResMut<ServerCameraView>,
    mut cameras: Query<&mut Transform, With<FlyCamera>>,
) {
    let Ok(mut transform) = cameras.single_mut() else {
        return;
    };
    let dt = time.delta_secs();
    let base = *transform;
    let stream = client_world
        .as_ref()
        .and_then(|world| world.stream.as_ref());
    let actors = |unique_id: i64| {
        let (position, yaw, pitch) = stream?.authority().actor_pose_by_unique(unique_id)?;
        Some(ActorView {
            position: Vec3::from_array(position),
            yaw_degrees: yaw,
            pitch_degrees: pitch,
        })
    };
    let context = ViewContext {
        base,
        subject: Transform {
            translation: view.eye_translation(),
            rotation: view.rotation(),
            ..Transform::IDENTITY
        },
        base_fov: settings.horizontal_fov_degrees(),
        actors: &actors,
    };
    if let Some(instructions) = instructions.as_deref() {
        server.observe_resets(instructions.resets());
        let seen = server.last_sequence();
        for entry in instructions.iter().filter(|entry| entry.sequence > seen) {
            server.apply(entry.sequence, &entry.event, &context);
        }
    }
    server.advance(dt);

    let override_pose = server.pose_override(&context);
    let mut pose = override_pose.unwrap_or(base);
    let mut changed = override_pose.is_some();

    if override_pose.is_none() && settings.perspective() == PerspectiveMode::FirstPerson {
        let nausea = Mat4::from_rotation_z(nausea_roll_radians(
            time.elapsed_secs(),
            vision.nausea,
            settings.feel().distortion_scale,
        ));
        let effect = hand.hurt * hand.bob.matrix() * nausea;
        if effect != Mat4::IDENTITY && effect.is_finite() {
            pose = Transform::from_matrix(pose.to_matrix() * effect.inverse());
            changed = true;
        }
    }

    let shake = server.shake_offset();
    if settings.feel().camera_shake
        && (shake.translation != Vec3::ZERO || shake.rotation != Quat::IDENTITY)
    {
        pose.translation += pose.rotation * shake.translation;
        pose.rotation = (pose.rotation * shake.rotation).normalize();
        changed = true;
    }

    if changed && pose.translation.is_finite() && pose.rotation.is_finite() {
        *transform = pose;
    }
}

#[cfg(test)]
mod tests {
    use bevy::prelude::{App, Update};
    use protocol::{CameraEvent, CameraInstructionEvent, CameraSetInstruction};

    use super::*;
    use crate::{camera::hurt::LocalHurtEvent, server_camera::ServerCameraInstructions};

    /// Runs the camera presentation core without a live world observation.
    #[allow(clippy::too_many_arguments)] // Each argument is a separately scheduled Bevy resource.
    fn present_camera(
        time: Res<Time>,
        settings: Res<CameraSettingsAuthority>,
        instructions: Option<Res<ServerCameraInstructions>>,
        hand: Res<FirstPersonHandMotion>,
        vision: Res<VisionEffects>,
        view: Res<LocalViewPose>,
        server: ResMut<ServerCameraView>,
        cameras: Query<&mut Transform, With<FlyCamera>>,
    ) {
        apply_camera_presentation(
            time,
            settings,
            instructions,
            hand,
            vision,
            view,
            None,
            server,
            cameras,
        );
    }

    fn camera_app() -> App {
        let mut app = App::new();
        app.init_resource::<Time>()
            .init_resource::<CameraSettingsAuthority>()
            .init_resource::<LocalViewPose>()
            .init_resource::<FirstPersonHandMotion>()
            .init_resource::<VisionEffects>()
            .init_resource::<ServerCameraView>()
            .add_systems(Update, present_camera);
        app.world_mut()
            .spawn((FlyCamera::default(), Transform::from_xyz(1.0, 2.0, 3.0)));
        app
    }

    fn camera_transform(app: &mut App) -> Transform {
        let mut query = app
            .world_mut()
            .query_filtered::<&Transform, With<FlyCamera>>();
        *query.single(app.world()).expect("one camera")
    }

    #[test]
    fn idle_presentation_leaves_the_transform_bit_identical() {
        let mut app = camera_app();
        app.update();
        assert_eq!(
            camera_transform(&mut app),
            Transform::from_xyz(1.0, 2.0, 3.0)
        );
    }

    #[test]
    fn hurt_tilt_rotates_the_first_person_camera() {
        let mut app = camera_app();
        let mut hurt = CameraHurtState::default();
        hurt.register(LocalHurtEvent::default());
        hurt.advance(0.08);
        app.world_mut().resource_mut::<FirstPersonHandMotion>().hurt = hurt.view_matrix(0.0);
        app.update();
        let transform = camera_transform(&mut app);
        assert!(transform.rotation.angle_between(Quat::IDENTITY) > 0.05);
    }

    #[test]
    fn server_set_instruction_overrides_the_camera_pose() {
        let mut app = camera_app();
        let mut instructions = ServerCameraInstructions::default();
        instructions.admit(
            1,
            0,
            [client_world::CommittedCameraEvent {
                sequence: 1,
                event: CameraEvent::Instruction(CameraInstructionEvent {
                    set: Some(CameraSetInstruction {
                        preset_id: 0,
                        ease: None,
                        position: Some([10.0, 20.0, 30.0]),
                        rotation_degrees: None,
                        facing_position: None,
                        view_offset: None,
                        entity_offset: None,
                        default_preset: None,
                        remove_ignore_starting_values: false,
                    }),
                    ..Default::default()
                }),
            }],
        );
        app.insert_resource(instructions);
        app.update();
        assert_eq!(
            camera_transform(&mut app).translation,
            Vec3::new(10.0, 20.0, 30.0)
        );
        app.update();
        assert_eq!(
            camera_transform(&mut app).translation,
            Vec3::new(10.0, 20.0, 30.0)
        );
    }

    #[test]
    fn vision_ramps_toward_goal_without_overshoot() {
        assert_eq!(approach(0.0, 1.0, 0.25), 0.25);
        assert_eq!(approach(0.9, 1.0, 0.25), 1.0);
        assert_eq!(approach(0.1, 0.0, 0.25), 0.0);
    }

    #[test]
    fn hand_motion_defaults_to_identity() {
        let hand = FirstPersonHandMotion::default();
        assert_eq!(hand.hurt * hand.bob.matrix(), Mat4::IDENTITY);
    }
}
