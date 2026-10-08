//! Stair corner selection from loaded neighboring block states.

use serde_json::{Map, Value};

use crate::{
    block_use::placement_cell,
    placement_state::{set_value, value},
};

// Front, back, left direction and blockers, then right direction and blockers.
const CORNERS: [[u8; 8]; 4] = [
    [3, 2, 3, 4, 5, 1, 5, 4],
    [4, 5, 0, 2, 3, 2, 3, 2],
    [2, 3, 1, 5, 4, 3, 4, 5],
    [5, 4, 2, 3, 2, 0, 2, 3],
];

/// Resolves a stair corner, refusing missing cells or unmodeled cornerable neighbors.
pub(crate) fn corner_states(
    identifier: &str,
    states: &mut Map<String, Value>,
    position: [i32; 3],
    mut read: impl FnMut([i32; 3]) -> Option<(String, Map<String, Value>)>,
) -> Option<()> {
    if !identifier.ends_with("_stairs") {
        return Some(());
    }
    let orientation = direction(states)?;
    let half = value(states, "upside_down_bit")?.as_u64()?;
    let config = CORNERS[orientation as usize];
    let mut corner = "none";
    'neighbors: for (face, blocker_index, names) in [
        (config[0], 3, ["outer_left", "outer_right"]),
        (config[1], 4, ["inner_left", "inner_right"]),
    ] {
        let (name, neighbor) = read(placement_cell(position, face))?;
        if !name.ends_with("_stairs") {
            if neighbor.contains_key("minecraft:corner") {
                return None;
            }
            continue;
        }
        if value(&neighbor, "upside_down_bit")?.as_u64()? != half {
            continue;
        }
        let neighbor_direction = direction(&neighbor)?;
        for side in 0..2 {
            if neighbor_direction != config[2 + side * 3] {
                continue;
            }
            let (blocker_name, mut blocker) =
                read(placement_cell(position, config[blocker_index + side * 3]))?;
            if blocker.contains_key("minecraft:corner") {
                set_value(&mut blocker, "minecraft:corner", Value::from("none"))?;
            }
            let mut straight = states.clone();
            set_value(&mut straight, "minecraft:corner", Value::from("none"))?;
            if blocker_name != identifier || blocker != straight {
                corner = names[side];
                break 'neighbors;
            }
        }
    }
    set_value(states, "minecraft:corner", Value::from(corner))
}

/// Converts the stair's legacy state to the shared south/west/north/east encoding.
fn direction(states: &Map<String, Value>) -> Option<u8> {
    [3, 1, 0, 2]
        .get(value(states, "weirdo_direction")?.as_u64()? as usize)
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Creates a typed stair state matching the palette's encoding.
    fn stair(direction: u8, upper: u8) -> Map<String, Value> {
        serde_json::from_value(json!({
            "weirdo_direction":{"type":"int","value": ([2,1,3,0][direction as usize])},
            "upside_down_bit":{"type":"byte","value":upper},
            "minecraft:corner":{"type":"string","value":"none"}
        }))
        .unwrap()
    }

    #[test]
    fn corners_follow_all_rotations_halves_and_blockers() {
        for (direction, face, neighbor_direction, blocker_face, expected) in [
            (0, 3, 3, 4, "outer_left"),
            (0, 3, 1, 5, "outer_right"),
            (0, 2, 3, 5, "inner_left"),
            (0, 2, 1, 4, "inner_right"),
            (1, 4, 0, 2, "outer_left"),
            (1, 4, 2, 3, "outer_right"),
            (1, 5, 0, 3, "inner_left"),
            (1, 5, 2, 2, "inner_right"),
            (2, 2, 1, 5, "outer_left"),
            (2, 2, 3, 4, "outer_right"),
            (2, 3, 1, 4, "inner_left"),
            (2, 3, 3, 5, "inner_right"),
            (3, 5, 2, 3, "outer_left"),
            (3, 5, 0, 2, "outer_right"),
            (3, 4, 2, 2, "inner_left"),
            (3, 4, 0, 3, "inner_right"),
        ] {
            for half in 0..2 {
                for blocked in [false, true] {
                    let mut center = stair(direction, half);
                    corner_states("minecraft:oak_stairs", &mut center, [0; 3], |position| {
                        if position == placement_cell([0; 3], face) {
                            Some((
                                "minecraft:birch_stairs".into(),
                                stair(neighbor_direction, half),
                            ))
                        } else if blocked && position == placement_cell([0; 3], blocker_face) {
                            Some(("minecraft:oak_stairs".into(), stair(direction, half)))
                        } else {
                            Some(("minecraft:air".into(), Map::new()))
                        }
                    })
                    .unwrap();
                    assert_eq!(
                        value(&center, "minecraft:corner"),
                        Some(&json!(if blocked { "none" } else { expected }))
                    );
                }
            }
        }
    }
}
