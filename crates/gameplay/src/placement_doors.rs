//! Paired door states and the neighboring facts used to choose their hinge.

use serde_json::{Map, Value};

use crate::placement_state::{only_keys, set_bit, set_value, value, yaw_quadrant};

/// Recognizes vanilla doors without including trapdoors or custom block behavior.
pub(crate) fn is_door(identifier: &str) -> bool {
    identifier.starts_with("minecraft:")
        && identifier.ends_with("_door")
        && !identifier.ends_with("_trapdoor")
}

/// Resolves both door halves; unknown solidity around the hinge defers the pair.
pub(crate) fn door_states(
    identifier: &str,
    canonical: &str,
    position: [i32; 3],
    yaw: f32,
    mut read: impl FnMut([i32; 3]) -> Option<String>,
) -> Option<(Map<String, Value>, Map<String, Value>)> {
    if !is_door(identifier) {
        return None;
    }
    let mut lower: Map<String, Value> = serde_json::from_str(canonical).ok()?;
    if !only_keys(
        &lower,
        &[
            "minecraft:cardinal_direction",
            "open_bit",
            "upper_block_bit",
            "door_hinge_bit",
        ],
    ) || !["south", "west", "north", "east"]
        .contains(&value(&lower, "minecraft:cardinal_direction")?.as_str()?)
    {
        return None;
    }
    let direction = (yaw_quadrant(yaw)? + 1) & 3;
    set_value(
        &mut lower,
        "minecraft:cardinal_direction",
        Value::from(["south", "west", "north", "east"][direction]),
    )?;
    set_bit(&mut lower, "upper_block_bit", false)?;
    set_bit(&mut lower, "door_hinge_bit", false)?;
    // Item data may carry the open bit; validate it without changing its value.
    let open = value(&lower, "open_bit")?;
    if !matches!(open.as_u64(), Some(0 | 1)) && !open.is_boolean() {
        return None;
    }
    let step = [[0, 0, 1], [-1, 0, 0], [0, 0, -1], [1, 0, 0]][direction];
    let mut sides = [(0, false); 2];
    for (side, sign) in [-1, 1].into_iter().enumerate() {
        for height in 0..2 {
            let name = read([
                position[0] + sign * step[0],
                position[1] + height,
                position[2] + sign * step[2],
            ])?;
            let solid = match name.as_str() {
                "minecraft:air" => false,
                "minecraft:stone" => true,
                name if is_door(name) => false,
                _ => return None,
            };
            sides[side].0 += u8::from(solid);
            sides[side].1 |= is_door(&name);
        }
    }
    let mut upper = lower.clone();
    set_bit(&mut upper, "upper_block_bit", true)?;
    set_bit(&mut upper, "door_hinge_bit", hinge(sides[0], sides[1]))?;
    Some((lower, upper))
}

/// A door only on the negative side forces the hinge; otherwise solid counts break ties.
fn hinge(negative: (u8, bool), positive: (u8, bool)) -> bool {
    (negative.1 && !positive.1) || negative.0 < positive.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hinge_table_covers_neighbor_doors_and_both_solid_counts() {
        const COUNTS: [[bool; 3]; 3] = [
            [false, true, true],
            [false, false, true],
            [false, false, false],
        ];
        for (negative, row) in COUNTS.into_iter().enumerate() {
            for (positive, expected) in row.into_iter().enumerate() {
                assert_eq!(
                    hinge((negative as u8, false), (positive as u8, false)),
                    expected
                );
                assert!(hinge((negative as u8, true), (positive as u8, false)));
                assert_eq!(
                    hinge((negative as u8, true), (positive as u8, true)),
                    expected
                );
                assert_eq!(
                    hinge((negative as u8, false), (positive as u8, true)),
                    expected
                );
            }
        }
    }
}
