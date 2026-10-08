use world::{
    BlockEntityNbt, BlockEntityNbtError, MAX_BLOCK_ENTITY_NBT_BYTES, MAX_NBT_COLLECTION_LENGTH,
    MAX_NBT_DEPTH, MAX_NBT_STRING_BYTES, RootByteCandidate,
};

fn var_u32(mut value: u32, out: &mut Vec<u8>) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if value == 0 {
            break;
        }
    }
}

fn zigzag_i32(value: i32, out: &mut Vec<u8>) {
    var_u32(((value << 1) ^ (value >> 31)) as u32, out);
}

fn string(value: &[u8], out: &mut Vec<u8>) {
    var_u32(value.len() as u32, out);
    out.extend_from_slice(value);
}

fn named_header(tag: u8, name: &[u8], out: &mut Vec<u8>) {
    out.push(tag);
    string(name, out);
}

fn representative_compound() -> Vec<u8> {
    let mut out = vec![10];
    string(b"", &mut out);

    named_header(8, b"id", &mut out);
    string(b"Chest", &mut out);
    for (name, value) in [
        (b"x".as_slice(), -17),
        (b"y".as_slice(), 64),
        (b"z".as_slice(), 31),
    ] {
        named_header(3, name, &mut out);
        zigzag_i32(value, &mut out);
    }

    named_header(9, b"Items", &mut out);
    out.push(10);
    zigzag_i32(1, &mut out);
    named_header(1, b"Count", &mut out);
    out.push(3);
    named_header(11, b"Ints", &mut out);
    zigzag_i32(2, &mut out);
    zigzag_i32(-1, &mut out);
    zigzag_i32(2, &mut out);
    out.push(0);

    named_header(7, b"Bytes", &mut out);
    zigzag_i32(3, &mut out);
    out.extend_from_slice(&[1, 2, 3]);
    named_header(12, b"Longs", &mut out);
    zigzag_i32(0, &mut out);
    out.push(0);
    out
}

fn idless_note_nbt(note: u8, powered: u8) -> Vec<u8> {
    let mut out = vec![10, 0];
    named_header(1, b"note", &mut out);
    out.push(note);
    named_header(1, b"powered", &mut out);
    out.push(powered);
    out.push(0);
    out
}

#[test]
fn idless_note_requires_bounded_typed_note_fields() {
    for (note, powered) in [(0, 0), (24, 1), (25, 2)] {
        let encoded = idless_note_nbt(note, powered);
        let (nbt, consumed) = BlockEntityNbt::decode_prefix(&encoded).expect("typed candidates");
        assert_eq!(consumed, encoded.len());
        assert_eq!(nbt.bytes(), encoded);
        assert_eq!(nbt.id(), None);
        assert_eq!(nbt.note_candidate(), RootByteCandidate::Value(note));
        assert_eq!(nbt.powered_candidate(), RootByteCandidate::Value(powered));
    }

    let (unrelated, _) = BlockEntityNbt::decode_prefix(&[10, 0, 0]).expect("empty compound");
    assert_eq!(unrelated.note_candidate(), RootByteCandidate::Absent);
    assert_eq!(unrelated.powered_candidate(), RootByteCandidate::Absent);
}

#[test]
fn note_candidates_record_wrong_types_and_duplicates_without_rejecting_other_ids() {
    let mut encoded = vec![10, 0];
    named_header(8, b"id", &mut encoded);
    string(b"Chest", &mut encoded);
    named_header(8, b"note", &mut encoded);
    string(b"extension", &mut encoded);
    named_header(1, b"note", &mut encoded);
    encoded.push(7);
    named_header(3, b"powered", &mut encoded);
    zigzag_i32(1, &mut encoded);
    named_header(1, b"powered", &mut encoded);
    encoded.push(1);
    encoded.push(0);

    let (nbt, consumed) = BlockEntityNbt::decode_prefix(&encoded).expect("extension fields decode");
    assert_eq!(consumed, encoded.len());
    assert_eq!(nbt.bytes(), encoded);
    assert_eq!(nbt.id(), Some("Chest"));
    assert_eq!(nbt.note_candidate(), RootByteCandidate::Invalid);
    assert_eq!(nbt.powered_candidate(), RootByteCandidate::Invalid);
}

#[test]
fn network_little_endian_prefix_retains_exact_bytes_and_root_identity() {
    let encoded = representative_compound();
    let mut payload = encoded.clone();
    payload.extend_from_slice(&[0xde, 0xad]);

    let (nbt, consumed) = BlockEntityNbt::decode_prefix(&payload).expect("decode compound prefix");

    assert_eq!(consumed, encoded.len());
    assert_eq!(nbt.bytes(), encoded.as_slice());
    assert_eq!(nbt.id(), Some("Chest"));
    assert_eq!(nbt.embedded_position(), Some([-17, 64, 31]));
}

#[test]
fn root_must_be_a_compound() {
    assert!(matches!(
        BlockEntityNbt::decode_prefix(&[1]),
        Err(BlockEntityNbtError::UnexpectedEof { .. })
    ));
    let error = BlockEntityNbt::decode_prefix(&[1, 0, 7]).unwrap_err();
    assert_eq!(error, BlockEntityNbtError::RootNotCompound { tag: 1 });

    let mut semantic_then_policy = vec![9, 0, 1];
    zigzag_i32(
        i32::try_from(MAX_NBT_COLLECTION_LENGTH + 1).unwrap(),
        &mut semantic_then_policy,
    );
    semantic_then_policy.resize(
        semantic_then_policy.len() + MAX_NBT_COLLECTION_LENGTH + 1,
        0,
    );
    assert_eq!(
        BlockEntityNbt::decode_prefix(&semantic_then_policy).unwrap_err(),
        BlockEntityNbtError::RootNotCompound { tag: 9 }
    );
}

#[test]
fn malformed_or_ambiguous_root_fields_are_rejected() {
    let mut duplicate_id = vec![10, 0];
    for value in [b"Chest".as_slice(), b"Barrel".as_slice()] {
        named_header(8, b"id", &mut duplicate_id);
        string(value, &mut duplicate_id);
    }
    duplicate_id.push(0);
    assert_eq!(
        BlockEntityNbt::decode_prefix(&duplicate_id).unwrap_err(),
        BlockEntityNbtError::DuplicateRootField { field: "id" }
    );

    let mut partial_position = vec![10, 0];
    named_header(3, b"x", &mut partial_position);
    zigzag_i32(1, &mut partial_position);
    partial_position.push(0);
    assert_eq!(
        BlockEntityNbt::decode_prefix(&partial_position).unwrap_err(),
        BlockEntityNbtError::PartialPosition
    );

    let mut semantic_then_policy = vec![10, 0];
    named_header(3, b"id", &mut semantic_then_policy);
    zigzag_i32(0, &mut semantic_then_policy);
    named_header(8, b"later", &mut semantic_then_policy);
    var_u32(
        u32::try_from(MAX_NBT_STRING_BYTES + 1).unwrap(),
        &mut semantic_then_policy,
    );
    semantic_then_policy.resize(semantic_then_policy.len() + MAX_NBT_STRING_BYTES + 1, 0);
    assert_eq!(
        BlockEntityNbt::decode_prefix(&semantic_then_policy).unwrap_err(),
        BlockEntityNbtError::InvalidRootFieldType {
            field: "id",
            expected: 8,
            actual: 3,
        }
    );
}

#[test]
fn invalid_utf8_and_nonempty_end_lists_fail_closed() {
    let mut invalid_utf8 = vec![10, 0];
    named_header(8, b"id", &mut invalid_utf8);
    string(&[0xff], &mut invalid_utf8);
    invalid_utf8.push(0);
    assert_eq!(
        BlockEntityNbt::decode_prefix(&invalid_utf8).unwrap_err(),
        BlockEntityNbtError::InvalidUtf8
    );

    let mut end_list = vec![10, 0];
    named_header(9, b"bad", &mut end_list);
    end_list.push(0);
    zigzag_i32(1, &mut end_list);
    end_list.push(0);
    assert_eq!(
        BlockEntityNbt::decode_prefix(&end_list).unwrap_err(),
        BlockEntityNbtError::NonEmptyEndList
    );
}

#[test]
fn decoder_enforces_depth_collection_string_and_byte_limits() {
    let mut nested = vec![10, 0];
    for _ in 0..=MAX_NBT_DEPTH {
        named_header(10, b"n", &mut nested);
    }
    nested.extend(std::iter::repeat_n(0, MAX_NBT_DEPTH + 2));
    assert!(matches!(
        BlockEntityNbt::decode_prefix(&nested),
        Err(BlockEntityNbtError::DepthExceeded { .. })
    ));

    let mut collection = vec![10, 0];
    named_header(7, b"a", &mut collection);
    zigzag_i32((MAX_NBT_COLLECTION_LENGTH + 1) as i32, &mut collection);
    collection.resize(collection.len() + MAX_NBT_COLLECTION_LENGTH + 1, 0);
    assert!(matches!(
        BlockEntityNbt::decode_prefix(&collection),
        Err(BlockEntityNbtError::CollectionTooLong { .. })
    ));

    let mut oversized_string = vec![10, 0];
    named_header(8, b"s", &mut oversized_string);
    var_u32((MAX_NBT_STRING_BYTES + 1) as u32, &mut oversized_string);
    oversized_string.resize(oversized_string.len() + MAX_NBT_STRING_BYTES + 1, 0);
    assert!(matches!(
        BlockEntityNbt::decode_prefix(&oversized_string),
        Err(BlockEntityNbtError::StringTooLong { .. })
    ));

    let mut oversized = vec![10, 0];
    while oversized.len() <= MAX_BLOCK_ENTITY_NBT_BYTES {
        named_header(7, b"a", &mut oversized);
        zigzag_i32(MAX_NBT_COLLECTION_LENGTH as i32, &mut oversized);
        oversized.resize(oversized.len() + MAX_NBT_COLLECTION_LENGTH, 0);
    }
    oversized.push(0);
    assert!(matches!(
        BlockEntityNbt::decode_prefix(&oversized),
        Err(BlockEntityNbtError::TooManyBytes { .. })
    ));
}

#[test]
fn oversized_declared_fields_prove_truncation_before_policy() {
    let string_len = MAX_NBT_STRING_BYTES + 1;
    let mut root_name = vec![10];
    var_u32(string_len as u32, &mut root_name);
    assert!(matches!(
        BlockEntityNbt::decode_prefix(&root_name),
        Err(BlockEntityNbtError::UnexpectedEof { .. })
    ));
    root_name.resize(root_name.len() + string_len, 0);
    assert!(matches!(
        BlockEntityNbt::decode_prefix(&root_name),
        Err(BlockEntityNbtError::StringTooLong { .. })
    ));

    let mut tag_name = vec![10, 0, 1];
    var_u32(string_len as u32, &mut tag_name);
    assert!(matches!(
        BlockEntityNbt::decode_prefix(&tag_name),
        Err(BlockEntityNbtError::UnexpectedEof { .. })
    ));
    tag_name.resize(tag_name.len() + string_len, 0);
    assert!(matches!(
        BlockEntityNbt::decode_prefix(&tag_name),
        Err(BlockEntityNbtError::StringTooLong { .. })
    ));

    let mut string_value = vec![10, 0];
    named_header(8, b"s", &mut string_value);
    var_u32(string_len as u32, &mut string_value);
    assert!(matches!(
        BlockEntityNbt::decode_prefix(&string_value),
        Err(BlockEntityNbtError::UnexpectedEof { .. })
    ));
    string_value.resize(string_value.len() + string_len, 0);
    assert!(matches!(
        BlockEntityNbt::decode_prefix(&string_value),
        Err(BlockEntityNbtError::StringTooLong { .. })
    ));

    let collection_len = MAX_NBT_COLLECTION_LENGTH + 1;
    for tag in [7_u8, 11, 12] {
        let mut truncated = vec![10, 0];
        named_header(tag, b"a", &mut truncated);
        zigzag_i32(collection_len as i32, &mut truncated);
        assert!(matches!(
            BlockEntityNbt::decode_prefix(&truncated),
            Err(BlockEntityNbtError::UnexpectedEof { .. })
        ));
        truncated.resize(truncated.len() + collection_len, 0);
        assert!(matches!(
            BlockEntityNbt::decode_prefix(&truncated),
            Err(BlockEntityNbtError::CollectionTooLong { .. })
        ));
    }

    for (element_tag, width) in [(2_u8, 2_usize), (3, 1)] {
        let mut truncated = vec![10, 0];
        named_header(9, b"l", &mut truncated);
        truncated.push(element_tag);
        zigzag_i32(collection_len as i32, &mut truncated);
        assert!(matches!(
            BlockEntityNbt::decode_prefix(&truncated),
            Err(BlockEntityNbtError::UnexpectedEof { .. })
        ));
        truncated.resize(truncated.len() + collection_len * width, 0);
        assert!(matches!(
            BlockEntityNbt::decode_prefix(&truncated),
            Err(BlockEntityNbtError::CollectionTooLong { .. })
        ));
    }
}

#[test]
fn aggregate_byte_limit_proves_truncation_before_policy() {
    let mut truncated = vec![10, 0];
    loop {
        let mut next = Vec::new();
        named_header(7, b"a", &mut next);
        zigzag_i32(MAX_NBT_COLLECTION_LENGTH as i32, &mut next);
        if truncated.len() + next.len() + MAX_NBT_COLLECTION_LENGTH > MAX_BLOCK_ENTITY_NBT_BYTES {
            truncated.extend(next);
            break;
        }
        truncated.extend(next);
        truncated.resize(truncated.len() + MAX_NBT_COLLECTION_LENGTH, 0);
    }
    assert!(matches!(
        BlockEntityNbt::decode_prefix(&truncated),
        Err(BlockEntityNbtError::UnexpectedEof { .. })
    ));
}

#[test]
fn truncated_and_overlong_varints_are_distinct_failures() {
    assert!(matches!(
        BlockEntityNbt::decode_prefix(&[10]),
        Err(BlockEntityNbtError::UnexpectedEof { .. })
    ));
    assert_eq!(
        BlockEntityNbt::decode_prefix(&[10, 0x80, 0x80, 0x80, 0x80, 0x80, 0]).unwrap_err(),
        BlockEntityNbtError::VarIntTooLong
    );

    let mut overlong_varlong = vec![10, 0];
    named_header(4, b"long", &mut overlong_varlong);
    overlong_varlong.extend(std::iter::repeat_n(0x80, 10));
    assert_eq!(
        BlockEntityNbt::decode_prefix(&overlong_varlong).unwrap_err(),
        BlockEntityNbtError::VarLongTooLong
    );

    let mut overflowing_varlong = vec![10, 0];
    named_header(4, b"long", &mut overflowing_varlong);
    overflowing_varlong.extend(std::iter::repeat_n(0x80, 9));
    overflowing_varlong.push(0x02);
    assert_eq!(
        BlockEntityNbt::decode_prefix(&overflowing_varlong).unwrap_err(),
        BlockEntityNbtError::VarLongOverflow
    );
}
