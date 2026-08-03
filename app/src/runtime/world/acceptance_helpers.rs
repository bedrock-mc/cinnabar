use client_world::CommittedControlEvent;

use crate::acceptance::{AcceptanceRun, markers::CAMERA_COMMITTED};
use crate::runtime::network::acceptance_surface_anchor;

pub(crate) fn refresh_mutation_anchor_from_committed_control(
    acceptance: &mut AcceptanceRun,
    control: &CommittedControlEvent,
) -> bool {
    let resolved = match control {
        CommittedControlEvent::MovePlayer { resolved, .. }
        | CommittedControlEvent::PlayerMovementCorrection { resolved, .. }
        | CommittedControlEvent::ChangeDimension { resolved, .. }
        | CommittedControlEvent::Respawn { resolved, .. } => resolved,
        CommittedControlEvent::SetTime { .. }
        | CommittedControlEvent::DaylightCycle { .. }
        | CommittedControlEvent::Weather { .. } => return false,
    };
    acceptance.refresh_mutation_surface_anchor(acceptance_surface_anchor(resolved.position))
}

pub(crate) fn model_gallery_camera_committed_marker(
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
