//! Initial local prediction and authority for an accepted StartGame session.

use super::{
    LocalPhysicsController, MovementSource, MovementTicker, PhysicsAuthorityFault,
    PhysicsAuthorityGate,
};

/// Provisional local pre-first-input anchor: public protocol documentation does
/// not define vanilla's initial `PlayerInputTick`, and StartGame world age is a
/// separate clock domain.
const START_GAME_PREDICTION_ANCHOR_TICK: u64 = 0;

/// Starts a session's prediction at its StartGame position, with no boat prediction carried
/// over from an earlier session.
pub fn reset_start_game_prediction(
    movement: &mut MovementTicker,
    local_physics: &mut LocalPhysicsController,
    session_generation: u64,
    initial_position: [f32; 3],
) {
    movement.reset(
        session_generation,
        START_GAME_PREDICTION_ANCHOR_TICK,
        initial_position,
    );
    local_physics.reanchor_network_position_before_advance(
        initial_position,
        START_GAME_PREDICTION_ANCHOR_TICK,
        false,
    );
    local_physics.end_vehicle_prediction();
}

/// Server facts and local policy needed to initialize player prediction.
pub struct StartGamePhysics {
    pub session_generation: u64,
    pub position: [f32; 3],
    pub rewind_history_size: i32,
    pub auto_fly: bool,
    pub collision_registry_complete: bool,
}

impl StartGamePhysics {
    /// Anchors prediction before selecting authority; a rejected authority leaves physics inactive.
    pub fn apply(
        self,
        gate: PhysicsAuthorityGate,
        movement: &mut MovementTicker,
        local_physics: &mut LocalPhysicsController,
    ) -> Result<MovementSource, PhysicsAuthorityFault> {
        movement.set_source(MovementSource::FreeCamera);
        local_physics.set_rewind_history_size(self.rewind_history_size);
        reset_start_game_prediction(
            movement,
            local_physics,
            self.session_generation,
            self.position,
        );
        match gate.apply_start_game(
            self.auto_fly,
            self.collision_registry_complete,
            movement,
            local_physics,
        ) {
            Ok(source) => Ok(source),
            Err(fault) => {
                movement.set_source(MovementSource::FreeCamera);
                local_physics.deactivate();
                Err(fault)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_replaces_prediction_before_selecting_authority() {
        for (gate, auto_fly, complete, expected) in [
            (
                PhysicsAuthorityGate::ProductionEnabled,
                false,
                true,
                Ok(MovementSource::Physics),
            ),
            (
                PhysicsAuthorityGate::ProductionEnabled,
                false,
                false,
                Err(PhysicsAuthorityFault::IncompleteCollisionRegistry),
            ),
            (
                PhysicsAuthorityGate::ProductionEnabled,
                true,
                false,
                Ok(MovementSource::FreeCamera),
            ),
            (
                PhysicsAuthorityGate::ProductionDisabled,
                false,
                false,
                Ok(MovementSource::FreeCamera),
            ),
        ] {
            let mut movement = MovementTicker::default();
            let mut physics = LocalPhysicsController::default();
            movement.reset(4, 100, [10.0, 72.62, 20.0]);
            movement.set_source(MovementSource::Physics);
            physics.reanchor_network_position([10.0, 72.62, 20.0], 100, false);
            let result = StartGamePhysics {
                session_generation: 5,
                position: [8.0, 70.62, -4.0],
                rewind_history_size: 42,
                auto_fly,
                collision_registry_complete: complete,
            }
            .apply(gate, &mut movement, &mut physics);

            assert_eq!(result, expected);
            assert_eq!(movement.session_generation(), 5);
            assert_eq!(
                movement.source(),
                expected.clone().unwrap_or(MovementSource::FreeCamera)
            );
            assert_eq!(physics.history_capacity(), 42);
            assert_eq!(physics.is_active(), expected == Ok(MovementSource::Physics));
            if let Some(state) = physics.state() {
                assert_eq!(state.tick, super::START_GAME_PREDICTION_ANCHOR_TICK);
                assert!((state.position.x - 8.0).abs() < f64::EPSILON);
                assert!((state.position.z + 4.0).abs() < f64::EPSILON);
            }
        }
    }
}
