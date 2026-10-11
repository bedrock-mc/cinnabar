use super::{BlockEntityNbt, NbtValue, ShapeActor, StateValue, state_identity};
use crate::NbtCompound;

/// Encodes an actor compound through the public wire ingestion boundary.
fn decode(root: NbtCompound) -> BlockEntityNbt {
    BlockEntityNbt::decode_prefix(&root.encode_root().unwrap())
        .unwrap()
        .0
}

/// Builds a typed piston snapshot with the supplied progress tag.
fn piston(progress: NbtValue) -> NbtCompound {
    let mut root = NbtCompound::default();
    root.insert("id", NbtValue::String("PistonArm".into()));
    root.insert("Progress", progress);
    root
}

#[test]
fn piston_progress_requires_a_finite_typed_float() {
    let nbt = decode(piston(NbtValue::Float(0.5)));
    assert_eq!(
        nbt.shape_actor(),
        &ShapeActor::Piston {
            progress_bits: 0.5_f32.to_bits()
        }
    );
    for value in [-0.1, 1.1, f32::NAN, f32::INFINITY] {
        assert_eq!(
            decode(piston(NbtValue::Float(value))).shape_actor(),
            &ShapeActor::None
        );
    }
    assert_eq!(
        decode(piston(NbtValue::Int(1))).shape_actor(),
        &ShapeActor::Piston {
            progress_bits: 0.0_f32.to_bits()
        }
    );
}

#[test]
fn moving_state_identity_preserves_types_and_expansion_requires_a_byte() {
    let mut states = NbtCompound::default();
    states.insert("age_bit", NbtValue::Byte(1));
    let mut block = NbtCompound::default();
    block.insert("name", NbtValue::String("minecraft:oak_sapling".into()));
    block.insert("states", NbtValue::Compound(states));
    let mut root = NbtCompound::default();
    root.insert("id", NbtValue::String("MovingBlock".into()));
    root.insert("movingBlock", NbtValue::Compound(block));
    root.insert("pistonPosY", NbtValue::Int(-1));
    root.insert("expanding", NbtValue::Int(1));
    let identity = state_identity("minecraft:oak_sapling", [("age_bit", StateValue::Byte(1))]);
    assert_eq!(
        decode(root.clone()).shape_actor(),
        &ShapeActor::Moving {
            state_identity: identity,
            piston: [0, -1, 0],
            expanding: false
        }
    );
    root.insert("expanding", NbtValue::Byte(1));
    assert_eq!(
        decode(root).shape_actor(),
        &ShapeActor::Moving {
            state_identity: identity,
            piston: [0, -1, 0],
            expanding: true
        }
    );
    assert_ne!(
        identity,
        state_identity("minecraft:oak_sapling", [("age_bit", StateValue::Int(1))])
    );
}
