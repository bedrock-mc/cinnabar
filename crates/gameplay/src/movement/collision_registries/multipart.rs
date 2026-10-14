//! The other block of two-block structures: door halves, double plants, pitcher crops and bed
//! ends, as the native client's `getSecondPart` overrides report them.

use assets::facts::BlockProperty as P;
use serde_json::{Map, Value};

/// Resolves the other-part offset for a state admitted by the compiled multipart trait.
pub(super) fn second_part_offset(
    fact: assets::facts::BlockFact,
    states: &Map<String, Value>,
) -> Option<[i32; 3]> {
    let value = |name: &str| states.get(name)?.get("value");
    if let Some(upper) = value(P::UpperBlockBit.name()) {
        if !fact
            .traits
            .contains(assets::registry::BlockTraits::MULTIPART)
        {
            return None;
        }
        return Some([0, if upper.as_i64()? != 0 { -1 } else { 1 }, 0]);
    }
    if !fact
        .traits
        .contains(assets::registry::BlockTraits::MULTIPART)
    {
        return None;
    }
    let head = value(P::HeadPieceBit.name())?.as_i64()? != 0;
    // A bed's direction 0..=3 faces south, west, north and east; its head lies ahead of its foot.
    let toward_head = match value(P::Direction.name())?.as_u64()? {
        0 => [0, 0, 1],
        1 => [-1, 0, 0],
        2 => [0, 0, -1],
        3 => [1, 0, 0],
        _ => return None,
    };
    Some(toward_head.map(|axis| if head { -axis } else { axis }))
}
