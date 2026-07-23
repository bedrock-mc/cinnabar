//! Icon carrier round-trip, ordering, dedup, bounds, and tamper coverage.

use std::sync::Arc;

use assets::{
    IconEntry, IconSprite, MAX_ICON_ENTRIES, MAX_ICON_SIDE, RuntimeIconCatalog, encode_icon_catalog,
};

fn sprite(width: u16, height: u16, fill: u8) -> IconSprite {
    IconSprite {
        width,
        height,
        rgba8: Arc::from(vec![fill; usize::from(width) * usize::from(height) * 4]),
    }
}

fn entry(identifier: &str, metadata: u32, sprite: u32) -> IconEntry {
    IconEntry {
        identifier: identifier.into(),
        metadata,
        sprite,
    }
}

#[test]
fn catalog_round_trips_sprites_and_resolves_alias_and_metadata_lookups() {
    let sprites = [sprite(16, 16, 10), sprite(32, 32, 20)];
    // Two keys share sprite 0 (an alias), one metadata variant uses sprite 1.
    let entries = [
        entry("minecraft:apple", 0, 0),
        entry("minecraft:golden_apple", 0, 1),
        entry("minecraft:golden_apple", 1, 0),
    ];
    let bytes = encode_icon_catalog([7; 32], &sprites, &entries).unwrap();
    let catalog = RuntimeIconCatalog::decode(&bytes).unwrap();

    assert_eq!(catalog.source_manifest_sha256(), [7; 32]);
    assert_eq!(catalog.sprites().len(), 2);
    assert_eq!(catalog.entries().len(), 3);
    let apple = catalog.lookup("minecraft:apple", 0).unwrap();
    assert_eq!((apple.width, apple.height, apple.rgba8[0]), (16, 16, 10));
    let golden = catalog.lookup("minecraft:golden_apple", 0).unwrap();
    assert_eq!(golden.rgba8[0], 20);
    // The exact metadata variant wins; an unknown metadata falls back to 0.
    assert_eq!(
        catalog.lookup("minecraft:golden_apple", 1).unwrap().rgba8[0],
        10
    );
    assert_eq!(
        catalog.lookup("minecraft:golden_apple", 9).unwrap().rgba8[0],
        20
    );
    assert!(catalog.lookup("minecraft:missing", 0).is_none());
}

#[test]
fn unsorted_dangling_and_oversized_inputs_fail_closed_at_encode_time() {
    let sprites = [sprite(16, 16, 1)];
    // Unsorted keys.
    assert!(
        encode_icon_catalog(
            [0; 32],
            &sprites,
            &[entry("minecraft:b", 0, 0), entry("minecraft:a", 0, 0)],
        )
        .is_err()
    );
    // Duplicate (identifier, metadata).
    assert!(
        encode_icon_catalog(
            [0; 32],
            &sprites,
            &[entry("minecraft:a", 0, 0), entry("minecraft:a", 0, 0)],
        )
        .is_err()
    );
    // Dangling sprite reference.
    assert!(encode_icon_catalog([0; 32], &sprites, &[entry("minecraft:a", 0, 1)]).is_err());
    // Oversized sprite side.
    let side = u16::try_from(MAX_ICON_SIDE + 1).unwrap();
    assert!(encode_icon_catalog([0; 32], &[sprite(side, 16, 1)], &[]).is_err());
    // Pixel length mismatch.
    let torn = IconSprite {
        width: 16,
        height: 16,
        rgba8: Arc::from(vec![0u8; 4]),
    };
    assert!(encode_icon_catalog([0; 32], &[torn], &[]).is_err());
}

#[test]
fn tampered_truncated_and_stale_carriers_fail_closed_at_decode_time() {
    let bytes = encode_icon_catalog(
        [3; 32],
        &[sprite(16, 16, 5)],
        &[entry("minecraft:apple", 0, 0)],
    )
    .unwrap();

    // Any flipped payload byte breaks the envelope hash.
    let mut corrupted = bytes.clone();
    let flip = corrupted.len() / 2;
    corrupted[flip] ^= 0xff;
    assert!(RuntimeIconCatalog::decode(&corrupted).is_err());

    // Truncation fails closed.
    assert!(RuntimeIconCatalog::decode(&bytes[..bytes.len() - 1]).is_err());

    // A stale (unknown) version is rejected before any payload reads.
    let mut stale = bytes.clone();
    stale[8..12].copy_from_slice(&9u32.to_le_bytes());
    assert!(RuntimeIconCatalog::decode(&stale).is_err());

    // Nonzero reserved padding is noncanonical.
    let mut padded = bytes;
    padded[20] = 1;
    assert!(RuntimeIconCatalog::decode(&padded).is_err());
}

#[test]
fn entry_count_bound_is_enforced_at_encode_time() {
    let sprites = [sprite(1, 1, 0)];
    let mut entries = Vec::with_capacity(MAX_ICON_ENTRIES + 1);
    for index in 0..=MAX_ICON_ENTRIES {
        entries.push(entry(&format!("minecraft:item_{index:06}"), 0, 0));
    }
    assert!(encode_icon_catalog([0; 32], &sprites, &entries).is_err());
}
