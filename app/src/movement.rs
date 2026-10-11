//! Session and world adapters for the interaction movement resources.

mod prediction_sync;
mod runtime_system;
pub(crate) use prediction_sync::send_movement_prediction_sync;
pub(crate) use runtime_system::advance_local_physics;

#[cfg(test)]
mod integration_tests;
#[cfg(test)]
mod teleport_ack_wiring_tests;
