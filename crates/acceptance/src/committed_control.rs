use client_world::CommittedControlEvent;

use crate::{AcceptanceRun, markers::CAMERA_COMMITTED};
/// Selects the horizontal surface cell for the observed server position.
pub fn acceptance_surface_anchor(position: [f32; 3]) -> [i32; 2] {
    [position[0].floor() as i32, position[2].floor() as i32]
}

/// Refreshes the unresolved mutation anchor from the committed player position.
pub fn refresh_mutation_anchor_from_committed_control(
    acceptance: &mut AcceptanceRun,
    control: &CommittedControlEvent,
) -> bool {
    let resolved = match control {
        CommittedControlEvent::MovePlayer { resolved, .. }
        | CommittedControlEvent::PlayerMovementCorrection { resolved, .. }
        | CommittedControlEvent::ChangeDimension { resolved, .. }
        | CommittedControlEvent::Respawn { resolved, .. } => resolved,
        CommittedControlEvent::SetTime { .. }
        | CommittedControlEvent::WorldClocks { .. }
        | CommittedControlEvent::DaylightCycle { .. }
        | CommittedControlEvent::WeatherCycle { .. }
        | CommittedControlEvent::Weather { .. }
        | CommittedControlEvent::LocalMovementEffect { .. }
        | CommittedControlEvent::LocalMovementSpeed { .. }
        | CommittedControlEvent::LocalMovementFlags { .. }
        | CommittedControlEvent::NetworkStackLatency { .. }
        | CommittedControlEvent::LocalActorMotion { .. }
        | CommittedControlEvent::LocalHurt { .. }
        | CommittedControlEvent::PlayerListChanged { .. } => return false,
    };
    acceptance.refresh_mutation_surface_anchor(acceptance_surface_anchor(resolved.position))
}

/// Reports only committed MovePlayer controls for a configured model witness.
pub fn model_gallery_camera_committed_marker(
    configured: bool,
    control: &CommittedControlEvent,
) -> Option<String> {
    if !configured {
        return None;
    }
    let CommittedControlEvent::MovePlayer {
        sequence,
        movement,
        resolved,
        ..
    } = control
    else {
        return None;
    };
    let [x, y, z] = resolved.position;
    Some(format!(
        "{CAMERA_COMMITTED} sequence={sequence} position={x:.5},{y:.5},{z:.5} yaw={:.5} pitch={:.5}",
        movement.yaw, movement.pitch
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_clock_and_weather_cycle_controls_leave_mutation_anchor_unchanged() {
        let mut acceptance = AcceptanceRun::new(Some(1), None, false, false);
        let anchor = [12, -34];
        acceptance.set_mutation_surface_anchor(anchor);
        for control in [
            CommittedControlEvent::WorldClocks {
                sequence: 1,
                update: protocol::WorldClockUpdateEvent::Initialize(
                    protocol::WorldClockDefinition {
                        id: protocol::OVERWORLD_CLOCK_ID,
                        time: 6000,
                        paused: false,
                    },
                ),
            },
            CommittedControlEvent::WorldClocks {
                sequence: 2,
                update: protocol::WorldClockUpdateEvent::Sync(protocol::WorldClockState {
                    id: protocol::OVERWORLD_CLOCK_ID,
                    time: 12000,
                    paused: true,
                }),
            },
            CommittedControlEvent::WeatherCycle {
                sequence: 3,
                enabled: false,
            },
            CommittedControlEvent::WeatherCycle {
                sequence: 4,
                enabled: true,
            },
        ] {
            assert!(!refresh_mutation_anchor_from_committed_control(
                &mut acceptance,
                &control,
            ));
            assert_eq!(acceptance.mutation_surface_anchor(), Some(anchor));
            assert_eq!(model_gallery_camera_committed_marker(true, &control), None);
        }
    }
}
