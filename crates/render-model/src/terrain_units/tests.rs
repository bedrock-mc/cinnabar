use super::*;

#[test]
fn units_cover_every_quad_once_and_reject_tail_before_addressing() {
    for count in [
        0,
        1,
        QUADS_PER_UNIT - 1,
        QUADS_PER_UNIT,
        QUADS_PER_UNIT + 1,
        2 * QUADS_PER_UNIT,
        4097,
    ] {
        let units = TerrainUnit::build(17..17 + count, 3, [[0; 3], [16; 3]], Some(2), 91)
            .collect::<Vec<_>>();
        let mut visible = Vec::new();
        TerrainUnit::compact(&units, |_| true, &mut visible);
        let quads = visible
            .iter()
            .flat_map(|unit| {
                (0..QUADS_PER_UNIT)
                    .filter_map(move |quad| unit.quad(quad * 4).map(|(index, _)| index))
            })
            .collect::<Vec<_>>();
        assert_eq!(quads, (17..17 + count).collect::<Vec<_>>());
        for (unit, gpu) in units.iter().zip(&visible) {
            assert_eq!(
                (unit.origin_slot, unit.facing, unit.allocation_generation),
                (3, Some(2), 91)
            );
            assert!(gpu.quad(unit.quad_count * 4).is_none());
            assert_eq!(gpu.quad(3), Some((unit.stream_start, 3)));
        }
    }
    // A final unit at the end of the address space must not wrap its tail.
    assert_eq!(
        TerrainUnit::build(u32::MAX - 1..u32::MAX, 0, [[0; 3]; 2], None, 0).count(),
        1
    );
}

#[test]
fn compaction_preserves_stream_order_and_shards_never_drop_units() {
    let units = (0..6)
        .flat_map(|face| {
            TerrainUnit::build(
                face * 200..face * 200 + 129,
                face,
                [[0; 3], [16; 3]],
                Some(face as u8),
                4,
            )
        })
        .collect::<Vec<_>>();
    let mut visible = Vec::new();
    TerrainUnit::compact(
        &units,
        |unit| unit.facing.is_some_and(|face| face % 2 == 0),
        &mut visible,
    );
    assert_eq!(
        visible
            .iter()
            .flat_map(|unit| unit.stream_start..unit.stream_start + unit.quad_count)
            .collect::<Vec<_>>(),
        (0..3)
            .flat_map(|face| face * 400..face * 400 + 129)
            .collect::<Vec<_>>()
    );
    let ranges =
        shards(visible.len(), 3 * size_of::<VisibleTerrainUnit>() as u64).collect::<Vec<_>>();
    assert!(ranges.iter().all(|range| range.len() <= 3));
    assert_eq!(
        ranges
            .into_iter()
            .flat_map(|range| visible[range].iter().copied())
            .collect::<Vec<_>>(),
        visible
    );
}

#[test]
fn indexed_pages_bound_both_regions_and_keep_the_original_storage_slot_count() {
    for bytes in 0..4096 {
        let capacity = IndexedUnitPage::capacity_for(bytes);
        if capacity == 0 {
            assert!(bytes < IndexedUnitPage { capacity: 1 }.byte_size());
            continue;
        }
        let page = IndexedUnitPage { capacity };
        assert!(page.byte_size() <= bytes);
        assert!(
            IndexedUnitPage {
                capacity: capacity + 1
            }
            .byte_size()
                > bytes
        );
        assert_eq!(
            u64::from(page.header(capacity as u32)[0]) * size_of::<VisibleTerrainUnit>() as u64,
            page.reference_offset()
        );
        assert_eq!(page.header(capacity as u32)[1], capacity as u32);
        assert!(
            page.reference_offset() + capacity as u64 * size_of::<u32>() as u64 <= page.byte_size()
        );
    }
}

#[test]
fn hidden_units_reject_valid_quad_addresses() {
    let mut unit = TerrainUnit::build(17..20, 3, [[0; 3], [16; 3]], None, 1)
        .next()
        .unwrap()
        .descriptor();
    unit.hidden = 1;
    assert_eq!(unit.quad(0), None);
    unit.hidden = 0;
    assert_eq!(unit.quad(0), Some((17, 0)));
}
