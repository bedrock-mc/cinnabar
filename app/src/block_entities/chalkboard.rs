//! Chalkboard size, mounting and ownership from retained block-entity data.

use assets::block_entity_geometry::chalkboard::ChalkboardSize;
use render_model::ChalkboardModel;
use world::NbtCompound;

use super::state::BlockState;

/// Draws the complete model only at its base, avoiding one copy per occupied block.
pub(super) fn model(
    state: &BlockState,
    nbt: &NbtCompound,
    position: [i32; 3],
) -> Option<ChalkboardModel> {
    for (key, value) in ["BaseX", "BaseY", "BaseZ"].into_iter().zip(position) {
        if nbt.integer(key)? != i64::from(value) {
            return None;
        }
    }
    Some(ChalkboardModel {
        size: ChalkboardSize::from_nbt(nbt.integer("Size")?)?,
        direction: u8::try_from(state.int("direction")?).ok()?,
        on_ground: nbt.boolean("OnGround")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use world::NbtValue;

    /// Builds neutral block-entity data without account or world fixture dependencies.
    fn data(size: i32, on_ground: bool) -> NbtCompound {
        let mut nbt = NbtCompound::default();
        for (name, value) in [("BaseX", 4), ("BaseY", 20), ("BaseZ", -7), ("Size", size)] {
            nbt.insert(name, NbtValue::Int(value));
        }
        nbt.insert("OnGround", NbtValue::Byte(i8::from(on_ground)));
        nbt
    }

    #[test]
    fn chalkboard_base_owns_size_direction_and_mount() {
        for size in ChalkboardSize::ALL {
            for direction in 0..16 {
                for on_ground in [false, true] {
                    let nbt = data(size.index() as i32, on_ground);
                    let state = BlockState::parse(&format!(r#"{{"direction":{direction}}}"#));
                    let result = model(&state, &nbt, [4, 20, -7]).unwrap();
                    assert_eq!(
                        result,
                        ChalkboardModel {
                            size,
                            direction,
                            on_ground
                        }
                    );
                    assert!(model(&state, &nbt, [4, 21, -7]).is_none());
                    assert!(model(&state, &nbt, [5, 20, -7]).is_none());
                }
            }
        }
    }

    #[test]
    fn chalkboard_unusable_identity_does_not_draw_an_unrelated_model() {
        let state = BlockState::parse(r#"{"direction":1}"#);
        assert!(model(&state, &data(9, false), [4, 20, -7]).is_none());
        assert!(model(&state, &NbtCompound::default(), [4, 20, -7]).is_none());
    }
}
