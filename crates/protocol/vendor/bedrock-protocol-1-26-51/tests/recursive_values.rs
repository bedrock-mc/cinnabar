// Code generated from canonical protocol manifest v2. DO NOT EDIT.

use bedrock_protocol_1_26_51::wire::{self, Decode, Encode};

mod recursive_value_200 {
    use super::*;
    use bedrock_protocol_1_26_51::types::DynamicValue as Value;
    const LEAF: i64 = 0;
    const LIST: i64 = 5;
    const MAP: i64 = 6;
    const UNKNOWN: i64 = 7;

    // Encode the manifest's union selector without constructing a recursive value.
    fn tag(out: &mut Vec<u8>, value: i64) {
        out.extend(wire::I32LE(value as _).encode_to_vec().unwrap());
    }

    #[derive(Clone, Copy)]
    enum Shape {
        List,
        Map,
        Mixed,
    }

    // Construct bytes directly so an over-budget fixture never creates an overdeep owned tree.
    fn encoded(shape: Shape, depth: usize, leaf: i64) -> Vec<u8> {
        let mut out = Vec::new();
        for level in 0..depth {
            let map = match shape {
                Shape::List => false,
                Shape::Map => true,
                Shape::Mixed => level % 2 != 0,
            };
            tag(&mut out, if map { MAP } else { LIST });
            if map {
                out.extend(wire::VarUInt(1).encode_to_vec().unwrap());
                out.extend(wire::VarUInt(1).encode_to_vec().unwrap());
                out.push(b'k');
            } else {
                out.extend(wire::VarUInt(1).encode_to_vec().unwrap());
            }
        }
        tag(&mut out, leaf);
        out
    }

    #[test]
    fn recursive_lists_maps_and_mixed_values_obey_the_edge_budget() {
        for shape in [Shape::List, Shape::Map, Shape::Mixed] {
            for depth in [0, 1, wire::MAX_RECURSIVE_VALUE_DEPTH] {
                let bytes = encoded(shape, depth, LEAF);
                let value = Value::decode_exact(&bytes).unwrap();
                assert_eq!(value.encode_to_vec().unwrap(), bytes);
                let cloned = value.clone();
                assert_eq!(cloned, value);
                drop(value);
                drop(cloned);
            }
            let bytes = encoded(shape, wire::MAX_RECURSIVE_VALUE_DEPTH + 1, LEAF);
            assert_eq!(
                Value::decode_exact(&bytes),
                Err(wire::DecodeError::RecursiveDepthExceeded)
            );
        }
    }

    #[test]
    fn recursive_budget_returns_after_success_and_child_decode_errors() {
        for shape in [Shape::List, Shape::Map, Shape::Mixed] {
            let valid = encoded(shape, wire::MAX_RECURSIVE_VALUE_DEPTH, LEAF);
            let mut bytes = valid.clone();
            bytes.extend_from_slice(&valid);
            let mut reader = wire::Reader::new(&bytes);
            Value::decode(&mut reader).unwrap();
            Value::decode(&mut reader).unwrap();
            reader.expect_consumed().unwrap();

            let mut bytes = encoded(shape, wire::MAX_RECURSIVE_VALUE_DEPTH, UNKNOWN);
            bytes.extend_from_slice(&valid);
            let mut reader = wire::Reader::new(&bytes);
            assert!(matches!(
                Value::decode(&mut reader),
                Err(wire::DecodeError::UnknownVariant { .. })
            ));
            Value::decode(&mut reader).unwrap();
            reader.expect_consumed().unwrap();

            let mut bytes = encoded(shape, wire::MAX_RECURSIVE_VALUE_DEPTH + 1, LEAF);
            let refused_end = bytes.len();
            bytes.extend_from_slice(&valid);
            let mut reader = wire::Reader::new(&bytes);
            assert_eq!(
                Value::decode(&mut reader),
                Err(wire::DecodeError::RecursiveDepthExceeded)
            );
            // Framing owns recovery: discard the refused value's known remaining bytes.
            let remaining = refused_end - reader.position();
            reader.take(remaining).unwrap();
            Value::decode(&mut reader).unwrap();
            reader.expect_consumed().unwrap();
        }
    }
}
