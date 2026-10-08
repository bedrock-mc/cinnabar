use super::*;

impl ActorStore {
    /// Replaces remote velocity and seeds a previously unrotated arrow's launch orientation.
    pub(crate) fn apply_motion(&mut self, sequence: u64, event: protocol::ActorMotionEvent) {
        if let Some(actor) = self.actors.get_mut(&event.actor_runtime_id) {
            actor.velocity = event.motion;
            actor.status.native_velocity = event.motion;
            actor.movement_revision = sequence;
            let arrow = matches!(&actor.kind, ActorKind::Entity { identifier }
                if identifier.as_ref() == "minecraft:arrow");
            if arrow && actor.previous_pose.pitch == 0.0 && actor.previous_pose.yaw == 0.0 {
                let [x, y, z] = event.motion;
                let yaw = x.atan2(z).to_degrees();
                let pitch = y.atan2(x.hypot(z)).to_degrees();
                actor.yaw = yaw;
                actor.pitch = pitch;
                actor.previous_pose.yaw = yaw;
                actor.previous_pose.pitch = pitch;
                if actor.interpolation_ticks_remaining == 0 {
                    actor.received_pose.yaw = yaw;
                    actor.received_pose.pitch = pitch;
                }
            }
        }
    }
}
