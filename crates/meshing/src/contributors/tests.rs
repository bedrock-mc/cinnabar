use super::*;

#[test]
fn covered_snow_cannot_accumulate_a_third_primary() {
    let snow = ResolvedPaletteEntry {
        network_value: 1,
        sequential_id: Some(1),
        kind: VisualKind::Model,
        variant: BLOCK_VISUAL_VARIANT_TOP_SNOW,
        ..ResolvedPaletteEntry::DIAGNOSTIC
    };
    let plant = ResolvedPaletteEntry {
        network_value: 2,
        sequential_id: Some(2),
        kind: VisualKind::Cross,
        ..ResolvedPaletteEntry::DIAGNOSTIC
    };
    for order in [[snow, plant], [plant, snow]] {
        let mut contributors = ResolvedContributors::default();
        for entry in order {
            contributors.push(entry);
        }
        assert_eq!(
            contributors.primary_network_value(),
            Some(snow.network_value)
        );
        assert_eq!(
            contributors.covered_plant.unwrap().network_value,
            plant.network_value
        );
        contributors.push(plant);
        assert_eq!(contributors.primary_network_value(), None);
        assert!(contributors.covered_plant.is_none());
        assert_eq!(
            contributors.diagnostic_network_value(),
            Some(plant.network_value)
        );
    }
}
