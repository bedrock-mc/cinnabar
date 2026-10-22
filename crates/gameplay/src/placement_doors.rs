//! Paired door states and the neighboring facts used to choose their hinge.

use assets::facts::BlockProperty as P;
use assets::{
    facts::{BlockFact, SupportKind},
    registry::BlockTraits as T,
};
use serde_json::{Map, Value};

use crate::placement_state::{only_keys, set_bit, set_value, value, yaw_quadrant};

/// Resolves both door halves; unknown solidity around the hinge defers the pair.
pub(crate) fn door_states(
    fact: BlockFact,
    canonical: &str,
    position: [i32; 3],
    yaw: f32,
    mut read: impl FnMut([i32; 3]) -> Option<BlockFact>,
) -> Option<(Map<String, Value>, Map<String, Value>)> {
    if !fact.traits.contains(T::DOOR) {
        return None;
    }
    let mut lower: Map<String, Value> = serde_json::from_str(canonical).ok()?;
    if !only_keys(
        &lower,
        &[
            P::MinecraftCardinalDirection.name(),
            P::OpenBit.name(),
            P::UpperBlockBit.name(),
            P::DoorHingeBit.name(),
        ],
    ) || crate::placement_state::valid_property(
        fact,
        &lower,
        assets::facts::BlockProperty::MinecraftCardinalDirection,
    )
    .is_none()
    {
        return None;
    }
    let direction = (yaw_quadrant(yaw)? + 1) & 3;
    set_value(
        &mut lower,
        P::MinecraftCardinalDirection.name(),
        Value::from(["south", "west", "north", "east"][direction]),
    )?;
    set_bit(&mut lower, P::UpperBlockBit.name(), false)?;
    set_bit(&mut lower, P::DoorHingeBit.name(), false)?;
    // Item data may carry the open bit; validate it without changing its value.
    let open = value(&lower, P::OpenBit.name())?;
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
            let solid = if name.traits.intersects(T::AIR | T::DOOR) {
                false
            } else if name.support == SupportKind::Stone {
                true
            } else {
                return None;
            };
            sides[side].0 += u8::from(solid);
            sides[side].1 |= name.traits.contains(T::DOOR);
        }
    }
    let mut upper = lower.clone();
    set_bit(&mut upper, P::UpperBlockBit.name(), true)?;
    set_bit(
        &mut upper,
        P::DoorHingeBit.name(),
        hinge(sides[0], sides[1]),
    )?;
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
