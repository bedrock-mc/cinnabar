use std::time::Instant;

use crate::{
    acceptance::AcceptanceRun, camera::AutoFly, local_player::LocalViewPose,
    runtime::world::ClientWorld, semantic_controls::SemanticInputSnapshot,
};
use bevy::{
    log::debug,
    prelude::{EulerRot, Local, Res, ResMut, Time, Vec3},
    time::Real,
};
use protocol::PlayerInputMode;
use semantic_input::Action;

use super::physics::is_transient_collision_unavailability;
use super::{
    LocalMovementEffectTimeline, LocalMovementSpeedAuthority, LocalPhysicsController,
    MovementTicker, PhysicsAuthorityFault, PhysicsCollisionRegistries, PhysicsSampleContext,
    physics_movement_input,
};

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
) {
    if acceptance.deadline_reached(Instant::now()) {
        movement_ticker.begin_terminal_drain();
    }
    if auto_fly.enabled() || !physics.is_active() {
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
    let (bevy_yaw, bevy_pitch, _) = view.rotation().to_euler(EulerRot::YXZ);
    let yaw = (180.0 - bevy_yaw.to_degrees()).rem_euclid(360.0);
    let mut input = physics_movement_input(
        movement,
        yaw,
        active,
        input.phase(Action::Jump).held,
        input.phase(Action::Sneak).held,
        input.phase(Action::Sprint).held,
        input.phase(Action::Use).held,
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
        },
        &world,
        &mut *movement_effects,
    );
    let blocker = frame.blocked.as_ref().map(ToString::to_string);
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
    if let Some(position) = physics.render_eye_position() {
        view.set_eye_translation(Vec3::from_array(position));
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
    if frame.dropped_ticks != 0 {
        return Some(PhysicsAuthorityFault::PhysicsTickOverflow {
            due: frame.due_ticks,
            dropped: frame.dropped_ticks,
        });
    }

    let error = frame.blocked.as_ref()?;
    Some(PhysicsAuthorityFault::PhysicsSimulationError {
        due: frame.due_ticks,
        tick_index: frame.blocked_tick_index.unwrap_or(frame.completed_ticks),
        error: error.clone(),
    })
}
