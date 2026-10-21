//! A beacon's level counts complete pyramid layers of base blocks beneath it, up to four.

use super::beacon_pyramid_level;

const BEACON: [i32; 3] = [10, 64, -3];

/// A pyramid of `layers` complete layers, with `missing` cells left out.
fn pyramid(layers: i32, missing: &[[i32; 3]]) -> impl Fn([i32; 3]) -> Option<bool> + '_ {
    move |[x, y, z]| {
        let layer = BEACON[1] - y;
        Some(
            (1..=layers).contains(&layer)
                && (x - BEACON[0]).abs() <= layer
                && (z - BEACON[2]).abs() <= layer
                && !missing.contains(&[x, y, z]),
        )
    }
}

#[test]
fn complete_layers_set_the_level_and_a_gap_stops_the_count() {
    assert_eq!(beacon_pyramid_level(BEACON, pyramid(0, &[])), Some(0));
    assert_eq!(beacon_pyramid_level(BEACON, pyramid(1, &[])), Some(1));
    assert_eq!(beacon_pyramid_level(BEACON, pyramid(3, &[])), Some(3));
    assert_eq!(beacon_pyramid_level(BEACON, pyramid(6, &[])), Some(4));
    let corner = [BEACON[0] + 2, BEACON[1] - 2, BEACON[2] - 2];
    assert_eq!(beacon_pyramid_level(BEACON, pyramid(4, &[corner])), Some(1));
}

/// An unloaded section that decides the level leaves it unknown instead of truncating it.
#[test]
fn unloaded_pyramid_sections_leave_the_level_unknown() {
    let complete = pyramid(4, &[]);
    let unloaded_below =
        |cell: [i32; 3]| (cell[1] > BEACON[1] - 3).then(|| complete(cell)).flatten();
    assert_eq!(beacon_pyramid_level(BEACON, unloaded_below), None);
    // A known gap in a lower layer still settles the level before the unloaded section.
    let corner = [BEACON[0] + 1, BEACON[1] - 1, BEACON[2] + 1];
    let missing = [corner];
    let gapped = pyramid(4, &missing);
    let gapped_unloaded =
        |cell: [i32; 3]| (cell[1] > BEACON[1] - 3).then(|| gapped(cell)).flatten();
    assert_eq!(beacon_pyramid_level(BEACON, gapped_unloaded), Some(0));
}
