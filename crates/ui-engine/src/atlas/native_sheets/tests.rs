use super::*;

/// A resident original image prepared before measuring its sampling work.
fn resident(size: [u32; 2]) -> Sheet {
    let plan = Plan::new(size).unwrap();
    Sheet {
        size,
        bytes: plan.bytes,
        tiles: plan
            .tiles
            .into_iter()
            .map(|spec| Tile {
                page: UiTexturePage::owned(
                    spec.size,
                    vec![255; spec.size[0] as usize * spec.size[1] as usize * 4].into(),
                )
                .unwrap(),
                spec,
            })
            .collect(),
    }
}

#[test]
fn resident_source_regions_preserve_coverage_without_heap_allocations() {
    let sheet = resident([8192, 16]);
    let boundary = INTERIOR as f32 / sheet.size[0] as f32;
    for (uv, expected) in [
        (
            UvRect {
                u0: 0.01,
                v0: 0.25,
                u1: 0.02,
                v1: 0.75,
            },
            1,
        ),
        (
            UvRect {
                u0: boundary - 0.01,
                v0: 0.0,
                u1: boundary + 0.01,
                v1: 1.0,
            },
            2,
        ),
        (
            UvRect {
                u0: 2.0,
                v0: 1.5,
                u1: -1.0,
                v1: -0.5,
            },
            15,
        ),
    ] {
        let ((count, area), allocations) = crate::allocation_count::count(|| {
            sheet.quads(uv, [10.0, 20.0, 110.0, 100.0], 11).fold(
                (0, 0.0f32),
                |(count, area), quad| {
                    let size = sheet.tiles[quad.page - 11].spec.size;
                    for (axis, side) in size.into_iter().enumerate() {
                        assert!(quad.uv[axis] >= 0.0 && quad.uv[axis] <= side as f32);
                        assert!(quad.uv[axis + 2] >= 0.0 && quad.uv[axis + 2] <= side as f32);
                    }
                    (
                        count + 1,
                        area + (quad.dest[2] - quad.dest[0]) * (quad.dest[3] - quad.dest[1]),
                    )
                },
            )
        });
        assert_eq!(count, expected);
        assert!(
            (area - 8000.0).abs() < 0.01,
            "sampling must cover the complete destination"
        );
        assert_eq!(allocations, 0, "resident sampling must not allocate");
    }
}

#[test]
fn source_limit_regions_keep_clamped_and_constant_sampling_covered() {
    let sheet = resident([MAX_UI_NATIVE_SHEET_SIDE, 1]);
    for uv in [
        UvRect {
            u0: -1.0,
            v0: -1.0,
            u1: 2.0,
            v1: 2.0,
        },
        UvRect {
            u0: 0.5,
            v0: 0.5,
            u1: 0.5,
            v1: 0.5,
        },
    ] {
        let (area, allocations) = crate::allocation_count::count(|| {
            sheet
                .quads(uv, [0.0, 0.0, 100.0, 10.0], 0)
                .map(|quad| (quad.dest[2] - quad.dest[0]) * (quad.dest[3] - quad.dest[1]))
                .sum::<f32>()
        });
        assert!((area - 1000.0).abs() < 0.01);
        assert_eq!(allocations, 0);
    }
}

#[test]
fn inline_decode_stays_pending_until_collected() {
    let mut sheets = NativeSheets::default();
    let source = Source {
        bytes: Arc::from([0u8; 4].as_slice()),
        size: [16, 16],
        packed: [16, 16],
    };
    let job = Job {
        key: "first".into(),
        bytes: Arc::clone(&source.bytes),
        plan: Plan::new(source.size).unwrap(),
        cancelled: Arc::clone(&sheets.cancelled),
        done: sheets.sender.clone(),
    };
    sheets.dispatch(job, None);
    assert_eq!(sheets.pending.as_deref(), Some("first"));
    sheets.request("second", &source);
    assert!(sheets.collect());
    assert!(sheets.failed("first"), "undecodable bytes fail once");
    assert!(
        !sheets.failed("second"),
        "the sibling waited instead of decoding"
    );
    assert!(sheets.pending.is_none());
    assert!(!sheets.collect());
}
