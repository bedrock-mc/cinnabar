use super::support::*;

#[test]
fn flipbook_preserves_complete_metadata_defaults_and_order() {
    let directory = pack_with_flipbooks(
        r#"[
            {
                "flipbook_texture": "textures/blocks/water",
                "atlas_tile": "water",
                "ticks_per_frame": 3,
                "frames": [7, 2, 9],
                "atlas_index": 4,
                "atlas_tile_variant": 6,
                "replicate": 2,
                "blend_frames": true
            },
            {
                "flipbook_texture": "textures/blocks/lava",
                "atlas_tile": "lava"
            }
        ]"#,
    );

    let pack = read_pack(directory.path()).expect("valid complete flipbook metadata");

    assert_eq!(pack.flipbooks.len(), 2);
    let water = &pack.flipbooks[0];
    assert_eq!(&*water.texture_path, "textures/blocks/water");
    assert_eq!(&*water.atlas_tile, "water");
    assert_eq!(water.ticks_per_frame, 3);
    assert_eq!(&*water.frames, &[7, 2, 9]);
    assert_eq!(water.atlas_index, 4);
    assert_eq!(water.atlas_tile_variant, 6);
    assert_eq!(water.replicate, 2);
    assert!(water.blend_frames);

    let lava = &pack.flipbooks[1];
    assert_eq!(&*lava.texture_path, "textures/blocks/lava");
    assert_eq!(&*lava.atlas_tile, "lava");
    assert_eq!(lava.ticks_per_frame, 1);
    assert!(lava.frames.is_empty());
    assert_eq!(lava.atlas_index, 0);
    assert_eq!(lava.atlas_tile_variant, 0);
    assert_eq!(lava.replicate, 1);
    assert!(!lava.blend_frames);
}

#[test]
fn flipbook_rejects_zero_timing_and_replication() {
    for (field, extra) in [
        ("ticks_per_frame", r#", "ticks_per_frame": 0"#),
        ("replicate", r#", "replicate": 0"#),
    ] {
        let flipbooks = format!(
            r#"[{{
                "flipbook_texture": "textures/blocks/water",
                "atlas_tile": "water"{extra}
            }}]"#
        );
        let directory = pack_with_flipbooks(&flipbooks);

        assert!(matches!(
            read_pack(directory.path()),
            Err(AssetError::ZeroFlipbookValue {
                index: 0,
                field: actual,
            }) if actual == field
        ));
    }
}

#[test]
fn flipbook_rejects_negative_and_non_integer_frame_values() {
    for invalid in ["-1", "1.5", r#""zero""#] {
        let flipbooks = format!(
            r#"[{{
                "flipbook_texture": "textures/blocks/water",
                "atlas_tile": "water",
                "frames": [0, {invalid}]
            }}]"#
        );
        let directory = pack_with_flipbooks(&flipbooks);

        assert!(matches!(
            read_pack(directory.path()),
            Err(AssetError::InvalidFlipbookInteger {
                index: 0,
                field: "frames",
                element: Some(1),
            })
        ));
    }
}

#[test]
fn flipbook_rejects_out_of_range_numeric_metadata() {
    for field in [
        "ticks_per_frame",
        "atlas_index",
        "atlas_tile_variant",
        "replicate",
    ] {
        let flipbooks = format!(
            r#"[{{
                "flipbook_texture": "textures/blocks/water",
                "atlas_tile": "water",
                "{field}": 4294967296
            }}]"#
        );
        let directory = pack_with_flipbooks(&flipbooks);

        assert!(matches!(
            read_pack(directory.path()),
            Err(AssetError::InvalidFlipbookInteger {
                index: 0,
                field: actual,
                element: None,
            }) if actual == field
        ));
    }
}

#[test]
fn flipbook_rejects_wrong_metadata_types() {
    for (field, extra, expected) in [
        (
            "ticks_per_frame",
            r#""ticks_per_frame": "one""#,
            "unsigned 32-bit integer",
        ),
        (
            "frames",
            r#""frames": {}"#,
            "array of unsigned 32-bit integers",
        ),
        ("blend_frames", r#""blend_frames": 1"#, "boolean"),
    ] {
        let flipbooks = format!(
            r#"[{{
                "flipbook_texture": "textures/blocks/water",
                "atlas_tile": "water",
                {extra}
            }}]"#
        );
        let directory = pack_with_flipbooks(&flipbooks);

        assert!(matches!(
            read_pack(directory.path()),
            Err(AssetError::InvalidFlipbookFieldType {
                index: 0,
                field: actual,
                expected: actual_expected,
            }) if actual == field && actual_expected == expected
        ));
    }
}

#[test]
fn flipbook_rejects_explicit_null_for_every_optional_field() {
    for (field, expected) in [
        ("ticks_per_frame", "unsigned 32-bit integer"),
        ("frames", "array of unsigned 32-bit integers"),
        ("atlas_index", "unsigned 32-bit integer"),
        ("atlas_tile_variant", "unsigned 32-bit integer"),
        ("replicate", "unsigned 32-bit integer"),
        ("blend_frames", "boolean"),
    ] {
        let flipbooks = format!(
            r#"[{{
                "flipbook_texture": "textures/blocks/water",
                "atlas_tile": "water",
                "{field}": null
            }}]"#
        );
        let directory = pack_with_flipbooks(&flipbooks);

        assert!(matches!(
            read_pack(directory.path()),
            Err(AssetError::InvalidFlipbookFieldType {
                index: 0,
                field: actual,
                expected: actual_expected,
            }) if actual == field && actual_expected == expected
        ));
    }
}

#[test]
fn flipbook_canonicalizes_selector_defaults_before_duplicate_detection() {
    let directory = pack_with_flipbooks(
        r#"[
            {
                "flipbook_texture": "textures/blocks/water",
                "atlas_tile": "water"
            },
            {
                "flipbook_texture": "textures/blocks/lava",
                "atlas_tile": "water",
                "atlas_index": 0,
                "atlas_tile_variant": 0
            }
        ]"#,
    );

    assert!(matches!(
        read_pack(directory.path()),
        Err(AssetError::DuplicateFlipbookSelector {
            ref atlas_tile,
            atlas_index: 0,
            atlas_tile_variant: 0,
        }) if &**atlas_tile == "water"
    ));
}

#[test]
fn flipbook_rejects_excessive_explicit_frame_lists() {
    let frames = std::iter::repeat_n("0", MAX_FLIPBOOK_FRAMES + 1)
        .collect::<Vec<_>>()
        .join(",");
    let flipbooks = format!(
        r#"[{{
            "flipbook_texture": "textures/blocks/water",
            "atlas_tile": "water",
            "frames": [{frames}]
        }}]"#
    );
    let directory = pack_with_flipbooks(&flipbooks);

    assert!(matches!(
        read_pack(directory.path()),
        Err(AssetError::TooManyFlipbookFrames {
            index: 0,
            count,
            max,
        }) if count == MAX_FLIPBOOK_FRAMES + 1 && max == MAX_FLIPBOOK_FRAMES
    ));
}

#[test]
fn flipbook_rejects_excessive_global_list() {
    let entry = r#"{
        "flipbook_texture": "textures/blocks/water",
        "atlas_tile": "water"
    }"#;
    let flipbooks = std::iter::repeat_n(entry, MAX_FLIPBOOKS + 1)
        .collect::<Vec<_>>()
        .join(",");
    let directory = pack_with_flipbooks(&format!("[{flipbooks}]"));

    assert!(matches!(
        read_pack(directory.path()),
        Err(AssetError::TooManyFlipbooks { count, max })
            if count == MAX_FLIPBOOKS + 1 && max == MAX_FLIPBOOKS
    ));
}

#[test]
fn flipbook_rejects_timeline_arithmetic_overflow() {
    let directory = pack_with_flipbooks(
        r#"[{
            "flipbook_texture": "textures/blocks/water",
            "atlas_tile": "water",
            "ticks_per_frame": 4294967295,
            "frames": [0, 1]
        }]"#,
    );

    assert!(matches!(
        read_pack(directory.path()),
        Err(AssetError::FlipbookTimelineOverflow { index: 0 })
    ));
}

#[test]
fn flipbook_replication_is_spatial_not_temporal() {
    let directory = pack_with_flipbooks(
        r#"[{
            "flipbook_texture": "textures/blocks/water",
            "atlas_tile": "water",
            "ticks_per_frame": 2,
            "replicate": 4294967295
        }]"#,
    );

    let pack = read_pack(directory.path()).expect("spatial replication must not overflow timing");
    assert_eq!(pack.flipbooks[0].ticks_per_frame, 2);
    assert!(pack.flipbooks[0].frames.is_empty());
    assert_eq!(pack.flipbooks[0].replicate, u32::MAX);
}
