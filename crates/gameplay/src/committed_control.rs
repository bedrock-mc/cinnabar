//! Applies FIFO-committed server authority to local gameplay before the next physics tick.
//!
//! Notification callbacks run at their original points inside reconciliation. Presentation
//! and evidence adapters can observe the change without owning prediction decisions.

use crate::movement::{
    self, LocalMovementEffectTimeline, LocalMovementSpeedAuthority, LocalPhysicsController,
    MovementTicker, PhysicsCorrectionMode, PhysicsCorrectionOutcome, ServerTeleportKind,
    reconcile_candidate_physics_correction, reconcile_committed_correction,
};
use client_world::CommittedControlEvent;
use sim::CollisionWorld;
use tracing::{debug, warn};

/// The local interpolation boundary requested by a committed spatial change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpatialReset {
    Correction,
    Dimension,
}

/// Whether the caller should publish environment state or reset its frame adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlDisposition {
    Handled,
    Environment,
    Spatial(SpatialReset),
}

/// Synchronous observations emitted before their corresponding downstream publication.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ControlObservation {
    Hurt {
        source_direction: Option<[f32; 2]>,
    },
    Knockback {
        motion: [f32; 3],
    },
    BeforeSpatial(CommittedControlEvent),
    Correction {
        outcome: PhysicsCorrectionOutcome,
        previous: [f32; 3],
        position: [f32; 3],
    },
    Dimension,
}

/// Mutable gameplay owners for one already committed control, without a second state copy.
pub struct CommittedGameplayState<'a> {
    pub movement: &'a mut MovementTicker,
    pub physics: &'a mut LocalPhysicsController,
    pub effects: &'a mut LocalMovementEffectTimeline,
    pub speed: &'a mut LocalMovementSpeedAuthority,
    pub session_generation: u64,
    pub dimension: i32,
    pub dimension_transfer_active: bool,
}

impl CommittedGameplayState<'_> {
    /// Applies authority and reports observations in the existing per-control order.
    /// The caller drains transport latency controls before entering this method.
    pub fn apply(
        &mut self,
        control: CommittedControlEvent,
        world: &impl CollisionWorld,
        mut observe: impl FnMut(ControlObservation),
    ) -> ControlDisposition {
        if let CommittedControlEvent::LocalHurt {
            source_direction, ..
        } = control
        {
            observe(ControlObservation::Hurt { source_direction });
            return ControlDisposition::Handled;
        }
        if matches!(
            control,
            CommittedControlEvent::PlayerListChanged { .. }
                | CommittedControlEvent::DimensionChangeAck { .. }
        ) {
            return ControlDisposition::Handled;
        }
        if let CommittedControlEvent::LocalMovementEffect { sequence, event } = control {
            self.effects.apply(self.session_generation, sequence, event);
            return ControlDisposition::Handled;
        }
        if let CommittedControlEvent::LocalMovementSpeed {
            sequence,
            dimension,
            current,
            sprint_modifier,
            tick,
        } = control
        {
            if self.speed.apply(
                self.session_generation,
                sequence,
                dimension,
                current,
                sprint_modifier,
            ) && self.movement.physics_is_authorized()
                && let Some((rewind, speed)) =
                    self.physics
                        .retime_movement_speed(tick, current, sprint_modifier)
            {
                self.speed.adopt_replayed_speed(speed);
                if let Some(rewind) = rewind {
                    replay_timeline_edit(self.movement, self.physics, rewind, world);
                }
            }
            return ControlDisposition::Handled;
        }
        if let CommittedControlEvent::LocalLiquidMovementSpeeds {
            sequence,
            dimension,
            underwater,
            lava,
            tick,
        } = control
        {
            if let Some(speeds) = self.speed.apply_liquid(
                self.session_generation,
                sequence,
                dimension,
                underwater,
                lava,
            ) && self.movement.physics_is_authorized()
                && let Some(rewind) = self.physics.retime_liquid_movement_speeds(tick, speeds)
            {
                replay_timeline_edit(self.movement, self.physics, rewind, world);
            }
            return ControlDisposition::Handled;
        }
        if let CommittedControlEvent::LocalMovementFlags { tick, flags, .. } = control {
            if self.movement.physics_is_authorized()
                && let Some(rewind) = self.physics.apply_server_movement_flags(tick, flags)
            {
                replay_timeline_edit(self.movement, self.physics, rewind, world);
            }
            return ControlDisposition::Handled;
        }
        if matches!(
            control,
            CommittedControlEvent::SetTime { .. }
                | CommittedControlEvent::WorldClocks { .. }
                | CommittedControlEvent::WeatherCycle { .. }
                | CommittedControlEvent::DaylightCycle { .. }
                | CommittedControlEvent::Weather { .. }
        ) {
            return ControlDisposition::Environment;
        }
        if let CommittedControlEvent::LocalMovementBoost { sequence, event } = control {
            // Vanilla predicts a boost from the tick the server stamped, so it
            // enters retained inputs and replays from there.
            if let Some(boost) = movement::MovementBoost::from_kind(event.kind) {
                let span = movement::BoostSpan::from_wire(event.duration_ticks);
                let (rewind, remaining) = if self.movement.physics_is_authorized() {
                    self.physics.retime_movement_boost(boost, event.tick, span)
                } else {
                    (None, Some(span))
                };
                self.effects.set_movement_boost(
                    self.session_generation,
                    sequence,
                    boost,
                    remaining,
                );
                if let Some(rewind) = rewind {
                    replay_timeline_edit(self.movement, self.physics, rewind, world);
                }
            }
            return ControlDisposition::Handled;
        }
        if let CommittedControlEvent::LocalActorMotion { event, .. } = control {
            // A server-driven impulse (knockback, explosion) must enter the
            // prediction timeline; without it the client keeps its pre-hit
            // trajectory and fights corrections after every hit. It needs no
            // spatial reconciliation and does not reset interpolation frames.
            observe(ControlObservation::Knockback {
                motion: event.motion,
            });
            if self.movement.physics_is_authorized() {
                movement::note_motion(event.tick, event.motion);
                if let Some(rewind) = self.physics.queue_server_motion(event.motion, event.tick)
                    && !replay_timeline_edit(self.movement, self.physics, rewind, world)
                {
                    self.physics.replace_live_velocity(event.motion);
                }
            }
            return ControlDisposition::Handled;
        }
        observe(ControlObservation::BeforeSpatial(control));
        let reset = match &control {
            CommittedControlEvent::PlayerMovementCorrection {
                correction,
                resolved,
                ..
            } => {
                if self.movement.physics_is_authorized() {
                    let previous = self.physics.network_position().unwrap_or(resolved.position);
                    // Shape classification (confirming / replay / teleport)
                    // lives with the movement authority; a confirming
                    // correction deliberately mutates no prediction state.
                    match movement::reconcile_prediction_correction(
                        self.movement,
                        self.physics,
                        resolved.position,
                        correction.tick,
                        correction.on_ground,
                        correction.delta,
                        world,
                    ) {
                        Ok(Some(outcome)) => {
                            observe(ControlObservation::Correction {
                                outcome,
                                previous,
                                position: resolved.position,
                            });
                            // Opt-in HandledTeleport acknowledgement dispatch
                            // (see the movement `teleport_ack` module).
                            self.movement.note_committed_correction_outcome(outcome);
                        }
                        Ok(None) => {}
                        Err(fault) => warn!(
                            ?fault,
                            correction_tick = correction.tick,
                            "local physics authority failed while applying a server correction"
                        ),
                    }
                } else {
                    self.movement
                        .snap_non_authoritative_anchor(correction.tick, resolved.position);
                    self.physics.reanchor_network_position_before_advance(
                        resolved.position,
                        correction.tick,
                        correction.on_ground,
                    );
                }
                SpatialReset::Correction
            }
            CommittedControlEvent::MovePlayer {
                movement: correction,
                resolved,
                ..
            } => {
                let tick = if self.dimension_transfer_active {
                    self.movement.completed_tick()
                } else {
                    correction.source_tick
                };
                if self.movement.physics_is_authorized() {
                    let previous = self.physics.network_position().unwrap_or(resolved.position);
                    // A teleport rewinds when nearby and retained, else snaps. An
                    // unmarked MovePlayer is classified like a correction (Cinnabar
                    // policy). HandledTeleport arms on the teleport path only.
                    let outcome = if correction.teleported {
                        self.movement
                            .note_server_teleport(ServerTeleportKind::MovePlayer);
                        movement::note_correction(
                            movement::CorrectionKind::Teleport,
                            tick,
                            resolved.position,
                            correction.on_ground,
                            self.physics.sample_at(tick),
                        );
                        movement::reconcile_move_player_teleport(
                            self.movement,
                            self.physics,
                            resolved.position,
                            tick,
                            correction.on_ground,
                            world,
                        )
                        .ok()
                    } else {
                        self.movement.note_unmarked_local_move_player();
                        reconcile_committed_correction(
                            self.movement,
                            self.physics,
                            resolved.position,
                            tick,
                            correction.on_ground,
                            None,
                            world,
                        )
                        .ok()
                        .flatten()
                    };
                    if let Some(outcome) = outcome {
                        observe(ControlObservation::Correction {
                            outcome,
                            previous,
                            position: resolved.position,
                        });
                    }
                } else {
                    self.movement
                        .snap_non_authoritative_anchor(tick, resolved.position);
                    self.physics.reanchor_network_position_before_advance(
                        resolved.position,
                        tick,
                        correction.on_ground,
                    );
                }
                SpatialReset::Correction
            }
            CommittedControlEvent::ChangeDimension { resolved, .. } => {
                let tick = self.movement.completed_tick();
                // Not a server teleport for HandledTeleport acknowledgement:
                // drop any armed assertion instead of leaking it across the
                // boundary.
                self.movement.clear_pending_teleport_ack();
                self.speed
                    .replace_dimension(self.session_generation, self.dimension);
                observe(ControlObservation::Dimension);
                if self.movement.physics_is_authorized() {
                    let previous = self.physics.network_position().unwrap_or(resolved.position);
                    if let Ok(outcome) = reconcile_candidate_physics_correction(
                        self.movement,
                        self.physics,
                        resolved.position,
                        tick,
                        false,
                        PhysicsCorrectionMode::Snap,
                        world,
                    ) {
                        observe(ControlObservation::Correction {
                            outcome,
                            previous,
                            position: resolved.position,
                        });
                    }
                } else {
                    self.movement
                        .snap_non_authoritative_anchor(tick, resolved.position);
                    self.physics.reanchor_network_position_before_advance(
                        resolved.position,
                        tick,
                        false,
                    );
                }
                SpatialReset::Dimension
            }
            CommittedControlEvent::Respawn {
                respawn, resolved, ..
            } => {
                if !respawn.ready_to_spawn() {
                    return ControlDisposition::Handled;
                }
                let tick = self.movement.completed_tick();
                if self.movement.physics_is_authorized() {
                    // Opt-in HandledTeleport acknowledgement: a committed
                    // respawn is a server-driven anchor, marked on admission
                    // under authority like the other qualifying sites (see
                    // the movement `teleport_ack` module).
                    self.movement
                        .note_server_teleport(ServerTeleportKind::Respawn);
                    let previous = self.physics.network_position().unwrap_or(resolved.position);
                    if let Ok(outcome) = reconcile_candidate_physics_correction(
                        self.movement,
                        self.physics,
                        resolved.position,
                        tick,
                        false,
                        PhysicsCorrectionMode::Snap,
                        world,
                    ) {
                        observe(ControlObservation::Correction {
                            outcome,
                            previous,
                            position: resolved.position,
                        });
                    }
                } else {
                    self.movement
                        .snap_non_authoritative_anchor(tick, resolved.position);
                    self.physics.reanchor_network_position_before_advance(
                        resolved.position,
                        tick,
                        false,
                    );
                }
                SpatialReset::Correction
            }
            CommittedControlEvent::SetTime { .. }
            | CommittedControlEvent::DimensionChangeAck { .. }
            | CommittedControlEvent::WorldClocks { .. }
            | CommittedControlEvent::WeatherCycle { .. }
            | CommittedControlEvent::DaylightCycle { .. }
            | CommittedControlEvent::Weather { .. }
            | CommittedControlEvent::LocalMovementEffect { .. }
            | CommittedControlEvent::LocalMovementSpeed { .. }
            | CommittedControlEvent::LocalLiquidMovementSpeeds { .. }
            | CommittedControlEvent::LocalMovementFlags { .. }
            | CommittedControlEvent::NetworkStackLatency { .. }
            | CommittedControlEvent::LocalActorMotion { .. }
            | CommittedControlEvent::LocalMovementBoost { .. }
            | CommittedControlEvent::LocalHurt { .. }
            | CommittedControlEvent::PlayerListChanged { .. } => {
                unreachable!(
                    "environment-only and impulse controls return before spatial reconciliation"
                )
            }
        };
        self.movement.enforce_local_physics_authority(self.physics);
        ControlDisposition::Spatial(reset)
    }
}

/// Replays a timeline edit; on failure the caller keeps the edit's live effect.
fn replay_timeline_edit(
    movement: &mut MovementTicker,
    physics: &mut LocalPhysicsController,
    rewind: u64,
    world: &impl CollisionWorld,
) -> bool {
    match movement::reconcile_timeline_rewind(movement, physics, rewind, world) {
        Ok(_) => true,
        Err(fault) => {
            debug!(
                ?fault,
                rewind, "timeline replay failed; the edit applies live only"
            );
            false
        }
    }
}

#[cfg(test)]
mod tests;
