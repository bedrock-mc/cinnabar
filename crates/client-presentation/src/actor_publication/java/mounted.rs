use super::{ActorSnapshot, WorldStream, lerp_degrees};

pub(super) fn body_yaw(
    stream: &WorldStream,
    rider: &ActorSnapshot,
    head_yaw: f32,
    alpha: f32,
) -> Option<f32> {
    let world = stream.authority();
    let mount = world.actor_by_unique_id(world.ridden_unique_id(rider.unique_id)?)?;
    if !mount.is_known_living_mount() {
        return None;
    }
    let rig = world.actor_rig(mount.runtime_id)?;
    let (body, alpha) = if matches!(mount.kind, protocol::ActorKind::Player { .. })
        && !rig.java.vanilla_posture
        && !render_model::is_pack_rig_id(render_model::EntityRigId(rig.rig.0))
    {
        (
            rig.java.body_yaw,
            rig.java.body_frame_alpha.unwrap_or(alpha),
        )
    } else {
        ([rig.previous_body_yaw, rig.body_yaw], alpha)
    };
    Some(frame_body_yaw(body, head_yaw, alpha))
}

pub(super) fn frame_body_yaw(body: [f32; 2], head_yaw: f32, alpha: f32) -> f32 {
    let body_yaw = lerp_degrees(body[0], body[1], alpha);
    client_world::java_mounted_body_yaw(body_yaw, head_yaw)
}
