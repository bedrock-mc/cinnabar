//! Retained oracle: Body-bound and per-type direction rules for vanilla placement.

use serde_json::{Map, Value};

use super::placement_state::{integer_in, only_keys, set_bit, set_value, yaw_quadrant};
use crate::block_use::BoxBounds;

/// Resolves six-facing blocks using loaded placement coordinates and player bounds.
pub(crate) fn facing_states(
    identifier: &str,
    canonical: &str,
    position: [i32; 3],
    player: BoxBounds,
    yaw: f32,
) -> Option<Map<String, Value>> {
    let (keys, rotation) = match identifier {
        "minecraft:dispenser" => (&["facing_direction", "triggered_bit"][..], 0.0),
        "minecraft:piston" | "minecraft:sticky_piston" => (&["facing_direction"][..], 180.0),
        _ => return None,
    };
    let mut states = serde_json::from_str(canonical).ok()?;
    if !only_keys(&states, keys) {
        return None;
    }
    integer_in(&states, "facing_direction", &[0, 1, 2, 3, 4, 5])?;
    let direction = body_facing(position, player, yaw, rotation)?;
    set_value(&mut states, "facing_direction", Value::from(direction))?;
    if keys.contains(&"triggered_bit") {
        set_bit(&mut states, "triggered_bit", false)?;
    }
    Some(states)
}

/// Nearby vertical placements use strict body bounds; other placements use the yaw table.
fn body_facing(position: [i32; 3], player: BoxBounds, yaw: f32, rotation: f32) -> Option<u8> {
    let player = (
        player.0.map(|value| value as f32),
        player.1.map(|value| value as f32),
    );
    if player.0.into_iter().chain(player.1).any(|v| !v.is_finite()) {
        return None;
    }
    let nearby = [0, 2].into_iter().all(|axis| {
        let coordinate = ((player.0[axis] + player.1[axis]) * 0.5).floor();
        (coordinate - position[axis] as f32).abs() < 2.0
    });
    let height = position[1] as f32;
    if nearby {
        if height < player.0[1] {
            return Some(1);
        }
        if player.1[1] < height {
            return Some(0);
        }
    }
    Some([2, 5, 3, 4][yaw_quadrant(yaw + rotation)?])
}
