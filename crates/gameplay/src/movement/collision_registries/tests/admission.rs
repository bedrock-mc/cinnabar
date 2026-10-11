use super::*;

const STATE_AXES: u32 = 16;

/// Appends a network-NBT string using its unsigned length encoding.
fn text(bytes: &mut Vec<u8>, value: &str) {
    let mut len = value.len();
    while len >= 128 {
        bytes.push((len as u8 & 127) | 128);
        len >>= 7;
    }
    bytes.push(len as u8);
    bytes.extend(value.as_bytes());
}

/// Appends a named network-NBT tag header.
fn tag(bytes: &mut Vec<u8>, kind: u8, name: &str) {
    bytes.push(kind);
    text(bytes, name);
}

/// Declares binary axes; large values exceed the byte budget before expansion.
fn large_definition(axes: u32, large_values: bool) -> Vec<u8> {
    let mut bytes = vec![10, 0];
    tag(&mut bytes, 9, "properties");
    bytes.extend([10, (axes * 2) as u8]);
    for index in 0..axes {
        tag(&mut bytes, 8, "name");
        text(&mut bytes, &format!("test:axis_{index}"));
        tag(&mut bytes, 9, "enum");
        if large_values {
            bytes.extend([8, 4]);
            for value in ["a", "b"] {
                text(&mut bytes, &value.repeat(1024));
            }
        } else {
            bytes.extend([1, 4, 0, 1]);
        }
        bytes.push(0);
    }
    bytes.push(0);
    bytes
}

#[test]
fn budget_rejections_preserve_vanilla_ids_in_both_modes_and_admission_palettes() {
    let version = active_content_registry_protocol();
    let records = assets::read_registry_for_protocol(BREG_V2193, version).unwrap();
    let mut registries = bind(
        BREG_V2193,
        &synthetic_preg(version, BREG_V2193, &records),
        version,
    )
    .unwrap();
    let first = records.len() as u32;
    let mut names = ["test:first", "test:middle", "test:last"];
    names.sort_by_key(|name| (protocol::block_name_sort_key(name), *name));
    let rejected_key = (protocol::block_name_sort_key(names[1]), names[1]);
    assert!(records.iter().any(|record| (
        protocol::block_name_sort_key(&record.name),
        record.name.as_ref()
    ) > rejected_key));
    for large_values in [false, true] {
        let axes = if large_values {
            STATE_AXES - 3
        } else {
            STATE_AXES
        };
        let large = large_definition(axes, large_values);
        let empty = [10, 0, 0];
        for supply_vanilla in [false, true] {
            let mut definitions = protocol::CustomBlocks::from_definitions([
                (names[0], empty.as_slice()),
                (names[1], large.as_slice()),
                (names[2], empty.as_slice()),
            ]);
            if supply_vanilla {
                definitions.vanilla_blocks = assets::server_defined_blocks()
                    .iter()
                    .map(|block| Arc::from(block.name))
                    .collect();
            }
            let (range, remap) = registries
                .begin_session_custom_blocks(&definitions)
                .unwrap();
            assert_eq!(
                registries.begin_session_hashed_custom_blocks(&definitions),
                Some(1)
            );
            for record in records
                .iter()
                .filter(|record| record.name.as_ref() != super::super::RESERVED_RECORD_NAME)
            {
                let omitted = !supply_vanilla
                    && assets::server_defined_blocks().iter().any(|block| {
                        (block.first_internal_id..block.first_internal_id + block.state_count)
                            .contains(&record.sequential_id)
                    });
                if omitted {
                    assert_eq!(remap.to_wire(record.sequential_id), u32::MAX);
                    continue;
                }
                let key = (
                    protocol::block_name_sort_key(&record.name),
                    record.name.as_ref(),
                );
                let customs_before = names
                    .iter()
                    .zip([1, 1 << axes, 1])
                    .filter(|(name, _)| (protocol::block_name_sort_key(name), **name) < key)
                    .map(|(_, count)| count)
                    .sum::<u32>();
                let omitted_before = if supply_vanilla {
                    0
                } else {
                    assets::server_defined_blocks()
                        .iter()
                        .filter(|block| block.first_internal_id < record.sequential_id)
                        .map(|block| block.state_count)
                        .sum::<u32>()
                };
                let expected_wire = record.sequential_id - omitted_before + customs_before;
                assert_eq!(
                    remap.to_internal(expected_wire),
                    record.sequential_id,
                    "{}",
                    record.name
                );
                assert_eq!(
                    remap.to_wire(record.sequential_id),
                    expected_wire,
                    "{}",
                    record.name
                );
                for (mode, id) in [
                    (
                        assets::NetworkIdMode::Sequential,
                        remap.to_internal(expected_wire),
                    ),
                    (assets::NetworkIdMode::Hashed, record.network_hash),
                ] {
                    assert_eq!(
                        registries.block_identifier(mode, id),
                        Some(record.name.as_ref())
                    );
                }
            }
            assert_eq!(definitions.blocks.len(), 3);
            assert_eq!(definitions.skipped, 2);
            assert_eq!(range, first..first + definitions.total_states());
            assert!(
                registries
                    .registry(assets::NetworkIdMode::Sequential)
                    .contains_runtime_id(first)
            );
            for id in [first + 1, range.end - 1] {
                assert!(
                    !registries
                        .registry(assets::NetworkIdMode::Sequential)
                        .contains_runtime_id(id)
                );
            }
        }
    }
}
