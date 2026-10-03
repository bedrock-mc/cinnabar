use std::time::Instant;

use crate::{
    acceptance::AcceptanceRun, camera::AutoFly, local_player::LocalViewPose,
    player_runtime::PlayerRuntime, runtime::world::ClientWorld,
    semantic_controls::SemanticInputSnapshot, settings_runtime::RuntimeSettings,
};
use bevy::{
    log::debug,
    prelude::{EulerRot, Local, Res, ResMut, Time, Vec3},
    time::Real,
};
use protocol::PlayerInputMode;
use semantic_input::Action;

use super::control_modes::{ControlModes, ControlObservation, DoubleTap};
use super::physics::is_transient_collision_unavailability;
use super::{
    LocalMovementEffectTimeline, LocalMovementSpeedAuthority, LocalPhysicsController, ModeIntent,
    MovementTicker, PhysicsAuthorityFault, PhysicsCollisionRegistries, PhysicsSampleContext,
    local_facts, physics_movement_input,
};

/// Frame-persistent sprint/sneak latches and the flight double-tap detector.
#[derive(Default)]
pub(crate) struct LocomotionLocals {
    controls: ControlModes,
    fly_tap: DoubleTap,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn advance_local_physics(
    time: Res<Time<Real>>,
    input: Res<SemanticInputSnapshot>,
    auto_fly: Res<AutoFly>,
    client_world: Res<ClientWorld>,
    collisions: Res<PhysicsCollisionRegistries>,
    acceptance: Res<AcceptanceRun>,
    mut physics: ResMut<LocalPhysicsController>,
    mut movement_effects: ResMut<LocalMovementEffectTimeline>,
    movement_speed: Res<LocalMovementSpeedAuthority>,
    mut movement_ticker: ResMut<MovementTicker>,
    mut view: ResMut<LocalViewPose>,
    mut previous_blocker: Local<Option<String>>,
    settings: Option<Res<RuntimeSettings>>,
    player: Option<Res<PlayerRuntime>>,
    item_use: Option<Res<crate::item_use::ItemUseRuntime>>,
    mut locals: Local<LocomotionLocals>,
) {
    if acceptance.deadline_reached(Instant::now()) {
        movement_ticker.begin_terminal_drain();
    }
    if auto_fly.enabled() || !physics.is_active() {
        locals.controls.reset();
        locals.fly_tap.reset();
        return;
    }
    if !movement_ticker.can_advance_physics_frame() {
        return;
    }
    let Some(stream) = client_world.stream.as_ref() else {
        return;
    };
    let semantic = input.snapshot();
    let active = semantic.is_some();
    let input_mode = semantic.map_or(PlayerInputMode::Mouse, |snapshot| {
        match snapshot.input_mode {
            semantic_input::InputMode::KeyboardMouse => PlayerInputMode::Mouse,
            semantic_input::InputMode::GamePad => PlayerInputMode::GamePad,
            semantic_input::InputMode::Touch => PlayerInputMode::Touch,
        }
    });
    let movement = input.movement();
    let raw_movement = input.raw_movement();
    let analogue_movement = input.analogue_movement();
    let (bevy_yaw, bevy_pitch, _) = view.rotation().to_euler(EulerRot::YXZ);
    let yaw = (180.0 - bevy_yaw.to_degrees()).rem_euclid(360.0);
    let facts = local_facts::read(
        player.as_deref(),
        stream,
        item_use
            .as_deref()
            .is_some_and(crate::item_use::ItemUseRuntime::is_using),
    );
    let gameplay = settings
        .as_deref()
        .map(|settings| settings.user_settings_update().1.gameplay)
        .unwrap_or_default();
    let now = time.elapsed();
    let jump = input.phase(Action::Jump);
    let sprint = input.phase(Action::Sprint);
    let sneak = input.phase(Action::Sneak);
    if !active {
        locals.controls.reset();
    }
    let fly_toggle = jump.pressed && locals.fly_tap.press(now);
    if let Some(server) = physics.take_server_control_flags() {
        locals
            .controls
            .adopt_server_flags(server.sprinting, server.sneaking);
    }
    let controlled = locals.controls.update(ControlObservation {
        now,
        forward: movement[1],
        sprint_pressed: sprint.pressed,
        sprint_held: sprint.held,
        sneak_pressed: sneak.pressed,
        sneak_held: sneak.held,
        toggle_sprint: gameplay.toggle_sprint,
        toggle_sneak: gameplay.toggle_sneak,
        sprint_blocked: facts.sprint_blocked,
        flying: physics.mode() == sim::MovementMode::Flying,
    });
    let mut input = physics_movement_input(
        movement,
        yaw,
        active,
        jump.held,
        controlled.sneaking,
        controlled.sprint_request,
        item_use
            .as_deref()
            .and_then(crate::item_use::ItemUseRuntime::movement_modifier),
    );
    input.movement_speed = movement_speed.current();
    let world = sim::PaletteWorld::new(
        stream.collision_store(),
        collisions.registry(stream.network_id_mode()),
        stream.current_dimension(),
    );
    let frame = physics.advance_with_context_and_effects(
        time.delta(),
        input,
        PhysicsSampleContext {
            pitch: -bevy_pitch.to_degrees(),
            head_yaw: yaw,
            camera_orientation: (view.rotation() * Vec3::NEG_Z).to_array(),
            input_mode,
            raw_move_vector: raw_movement,
            analogue_move_vector: analogue_movement,
            mode_intent: ModeIntent {
                ride: facts.ride,
                ride_seat: facts.ride_seat,
                can_fly: facts.can_fly,
                server_flying: facts.server_flying,
                fly_toggle,
                fly_speed: facts.fly_speed,
                vertical_fly_speed: facts.vertical_fly_speed,
                creative_flight: facts.creative_flight,
                elytra_ready: facts.elytra_ready,
                depth_strider: facts.depth_strider,
                soul_speed: facts.soul_speed,
            },
            sneak_button: active && sneak.held,
        },
        &world,
        &mut *movement_effects,
    );
    let blocker = frame.blocked.as_ref().map(ToString::to_string);
    if frame.dropped_ticks != 0 {
        // Time starvation keeps the retained samples contiguous and monotonic,
        // so the outbound stream remains a valid 20 Hz sequence and the server
        // can still reconcile it. Record the stall instead of permanently
        // revoking authority over a load symptom; a live Venity session died
        // exactly here when join-time streaming stalls dropped seven of
        // fifteen due ticks.
        debug!(
            due = frame.due_ticks,
            dropped = frame.dropped_ticks,
            "local physics dropped excess catch-up ticks"
        );
    }
    let authority_fault = physics_authority_fault_for_frame(&frame);
    if blocker != *previous_blocker {
        if authority_fault.is_none()
            && let Some(blocker) = blocker.as_deref()
        {
            debug!(%blocker, "local physics is waiting for authoritative collision data");
        }
        *previous_blocker = blocker;
    }
    if let Some(fault) = authority_fault
        && movement_ticker.physics_is_authorized()
    {
        movement_ticker.record_physics_fault(fault);
        physics.deactivate();
        return;
    }
    for sample in frame.samples {
        if let Err(fault) = movement_ticker.enqueue_completed_physics(sample) {
            debug!(?fault, "local Physics movement authority failed closed");
            physics.deactivate();
            return;
        }
    }
    if let (Some(eye), Some(feet)) = (
        physics.render_eye_position(),
        physics.render_feet_position(),
    ) {
        view.set_subject_position(Vec3::from_array(eye), Vec3::from_array(feet));
    }
}

pub(crate) fn physics_authority_fault_for_frame(
    frame: &super::LocalPhysicsFrame,
) -> Option<PhysicsAuthorityFault> {
    if frame
        .blocked
        .as_ref()
        .is_some_and(is_transient_collision_unavailability)
    {
        return None;
    }

    let error = frame.blocked.as_ref()?;
    Some(PhysicsAuthorityFault::PhysicsSimulationError {
        due: frame.due_ticks,
        tick_index: frame.blocked_tick_index.unwrap_or(frame.completed_ticks),
        error: error.clone(),
    })
}
