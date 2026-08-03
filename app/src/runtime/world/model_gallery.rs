use client_world::CommittedControlEvent;

use crate::acceptance::markers::CAMERA_COMMITTED;

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
