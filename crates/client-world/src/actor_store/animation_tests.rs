//! Actor authority driving the animation runtime, from packets and ticks through to rigs.
use std::collections::HashMap;

use protocol::{ActorKind, ActorMetadataValue};

use super::{ActorPose, ActorSnapshot};

mod creeper_tests;
mod dragon_tests;
mod hurt_tests;
mod render_frame_tests;
mod rig_input_tests;
mod server_tests;
mod skin_tests;

/// An entity snapshot at the origin with the given metadata.
fn actor_with_metadata(metadata: HashMap<u32, ActorMetadataValue>) -> ActorSnapshot {
    let pose = ActorPose {
        position: [0.0; 3],
        pitch: 0.0,
        yaw: 0.0,
        head_yaw: 0.0,
    };
    ActorSnapshot {
        unique_id: -1,
        runtime_id: 1,
        spawn_revision: 1,
        movement_revision: 0,
        kind: ActorKind::Entity {
            identifier: "minecraft:test".into(),
        },
        position: [0.0; 3],
        velocity: [0.0; 3],
        pitch: 0.0,
        yaw: 0.0,
        head_yaw: 0.0,
        previous_pose: pose,
        received_pose: pose,
        interpolation_ticks_remaining: 0,
        body_yaw: 0.0,
        on_ground: Some(false),
        teleported: false,
        player_mode: None,
        player_game_mode: None,
        source_tick: None,
        metadata,
        attributes: HashMap::new(),
        int_properties: HashMap::new(),
        float_properties: HashMap::new(),
        status: Default::default(),
        dragon_animation: None,
        hitboxes: None,
    }
}
