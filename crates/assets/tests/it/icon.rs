//! Icon carrier round-trip, ordering, dedup, bounds, and tamper coverage.

use std::sync::Arc;

use assets::{
    BLOCK_ITEM_FACE_SIDE, BLOCK_ITEM_SHEET_GRID, BLOCK_ITEM_SHEET_SIZE, BlockVisualId,
    IconBlockModel, IconBlockSheet, IconEntry, IconSprite, MAX_ICON_ENTRIES, MAX_ICON_SIDE,
    RuntimeIconCatalog, compose_block_item_sheet, encode_icon_catalog,
    encode_icon_catalog_with_block_sheets, encode_icon_catalog_with_blocks,
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

#[test]
fn carried_block_faces_round_trip_without_occupying_an_inventory_icon_key() {
    let tiles = std::array::from_fn(|face| {
        sprite(BLOCK_ITEM_FACE_SIDE, BLOCK_ITEM_FACE_SIDE, face as u8 + 1)
    });
    let sheet = compose_block_item_sheet(&tiles).unwrap();
    assert_eq!([sheet.width, sheet.height], BLOCK_ITEM_SHEET_SIZE);
    let sheets = [IconBlockSheet {
        visual: BlockVisualId(7),
        sprite: 1,
    }];
    let bytes = encode_icon_catalog_with_block_sheets(
        [7; 32],
        &[sprite(16, 16, 99), sheet],
        &[entry("test:carried_block", 0, 0)],
        &sheets,
    )
    .unwrap();
    let decoded = RuntimeIconCatalog::decode(&bytes).unwrap();
    assert_eq!(decoded.block_sheets(), sheets);
    assert_eq!(
        decoded.lookup("test:carried_block", 0).unwrap().rgba8[0],
        99
    );
    let sheet = &decoded.sprites()[decoded.block_sheets()[0].sprite as usize];
    let columns = usize::from(BLOCK_ITEM_SHEET_GRID[0]);
    let side = usize::from(BLOCK_ITEM_FACE_SIDE);
    for face in 0..6 {
        for (dx, dy) in [(0, 0), (side - 1, side - 1)] {
            let x = face % columns * side + dx;
            let y = face / columns * side + dy;
            let pixel = (y * usize::from(sheet.width) + x) * 4;
            assert_eq!(&sheet.rgba8[pixel..pixel + 4], &[face as u8 + 1; 4]);
        }
    }
}

#[test]
fn carried_block_sheet_order_references_and_dimensions_fail_closed() {
    let binding = |visual, sprite| IconBlockSheet {
        visual: BlockVisualId(visual),
        sprite,
    };
    let sprites = [sprite(
        BLOCK_ITEM_SHEET_SIZE[0],
        BLOCK_ITEM_SHEET_SIZE[1],
        1,
    )];
    for invalid in [
        vec![binding(7, 1)],
        vec![binding(7, 0), binding(7, 0)],
        vec![binding(8, 0), binding(7, 0)],
    ] {
        assert!(encode_icon_catalog_with_block_sheets([7; 32], &sprites, &[], &invalid).is_err());
    }
    assert!(
        encode_icon_catalog_with_block_sheets(
            [7; 32],
            &[sprite(16, 16, 1)],
            &[],
            &[binding(7, 0)],
        )
        .is_err()
    );
    let mut tiles = std::array::from_fn(|_| sprite(BLOCK_ITEM_FACE_SIDE, BLOCK_ITEM_FACE_SIDE, 1));
    tiles[3].rgba8 = Arc::from([1; 4]);
    assert!(compose_block_item_sheet(&tiles).is_none());
}

#[test]
fn block_model_thumbnails_round_trip_beside_carried_sheets() {
    let model = |sprite, visual| IconBlockModel {
        sprite,
        visual: BlockVisualId(visual),
    };
    let sheet = sprite(BLOCK_ITEM_SHEET_SIZE[0], BLOCK_ITEM_SHEET_SIZE[1], 3);
    let sprites = [sprite(32, 32, 1), sheet, sprite(32, 32, 2)];
    let sheets = [IconBlockSheet {
        visual: BlockVisualId(4),
        sprite: 1,
    }];
    let models = [model(0, 40), model(2, 9)];
    let entries = [entry("test:stairs", 0, 0), entry("test:wall", 0, 2)];
    for sheets in [&sheets[..], &[]] {
        let bytes =
            encode_icon_catalog_with_blocks([7; 32], &sprites, &entries, sheets, &models).unwrap();
        let decoded = RuntimeIconCatalog::decode(&bytes).unwrap();
        assert_eq!(decoded.block_models(), models);
        assert_eq!(decoded.block_sheets(), sheets);
        assert_eq!(decoded.lookup("test:wall", 0).unwrap().rgba8[0], 2);
    }
    // Without models the earlier layouts are unchanged.
    assert_eq!(
        encode_icon_catalog_with_blocks([7; 32], &sprites, &entries, &sheets, &[]).unwrap(),
        encode_icon_catalog_with_block_sheets([7; 32], &sprites, &entries, &sheets).unwrap()
    );
    let decoded =
        RuntimeIconCatalog::decode(&encode_icon_catalog([7; 32], &sprites, &entries).unwrap())
            .unwrap();
    assert!(decoded.block_models().is_empty());
    for invalid in [
        vec![model(3, 1)],
        vec![model(2, 1), model(0, 1)],
        vec![model(0, 1), model(0, 2)],
    ] {
        assert!(
            encode_icon_catalog_with_blocks([7; 32], &sprites, &entries, &[], &invalid).is_err()
        );
    }
}
