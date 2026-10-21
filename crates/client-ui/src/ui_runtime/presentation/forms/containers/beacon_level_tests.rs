//! A beacon's level counts complete pyramid layers of base blocks beneath it, up to four.

use super::beacon_pyramid_level;

const BEACON: [i32; 3] = [10, 64, -3];

/// A pyramid of `layers` complete layers, with `missing` cells left out.
fn pyramid(layers: i32, missing: &[[i32; 3]]) -> impl Fn([i32; 3]) -> bool + '_ {
    move |[x, y, z]| {
        let layer = BEACON[1] - y;
        (1..=layers).contains(&layer)
            && (x - BEACON[0]).abs() <= layer
            && (z - BEACON[2]).abs() <= layer
            && !missing.contains(&[x, y, z])
    }
}

#[test]
fn complete_layers_set_the_level_and_a_gap_stops_the_count() {
    assert_eq!(beacon_pyramid_level(BEACON, pyramid(0, &[])), 0);
    assert_eq!(beacon_pyramid_level(BEACON, pyramid(1, &[])), 1);
    assert_eq!(beacon_pyramid_level(BEACON, pyramid(3, &[])), 3);
    assert_eq!(beacon_pyramid_level(BEACON, pyramid(6, &[])), 4);
    let corner = [BEACON[0] + 2, BEACON[1] - 2, BEACON[2] - 2];
    assert_eq!(beacon_pyramid_level(BEACON, pyramid(4, &[corner])), 1);
}
