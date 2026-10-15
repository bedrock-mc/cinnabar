use far_terrain::{ColumnSummary, SummaryDecodeError, SurfaceCell};

pub fn hilly() -> ColumnSummary {
    let mut summary = ColumnSummary::default();
    for (index, cell) in summary.cells_mut().iter_mut().enumerate() {
        *cell = SurfaceCell {
            height: 60 + (index % 7) as i16 - (index / 40) as i16,
            rgb: [index as u8, 120, 255 - index as u8],
            water_depth: (index % 5 == 0) as u8 * 3,
        };
    }
    summary.cells_mut()[17] = SurfaceCell::EMPTY;
    summary
}

#[test]
fn encoding_round_trips_every_field_including_empty_cells() {
    let summary = hilly();
    let encoded = summary.encode();
    assert_eq!(ColumnSummary::decode(&encoded.bytes), Ok(summary.clone()));
    assert_eq!(encoded.hash, summary.content_hash());
    assert!(
        encoded.bytes.len() < 256 * 6,
        "{} bytes",
        encoded.bytes.len()
    );
}

#[test]
fn hash_tracks_content_only() {
    let summary = hilly();
    let mut changed = summary.clone();
    changed.cells_mut()[200].rgb[1] ^= 1;
    assert_eq!(summary.content_hash(), hilly().content_hash());
    assert_ne!(summary.content_hash(), changed.content_hash());
}

#[test]
fn decoding_rejects_unknown_formats_and_bad_lengths() {
    let mut bytes = hilly().encode().bytes.to_vec();
    assert_eq!(ColumnSummary::decode(&[]), Err(SummaryDecodeError::Length));
    assert_eq!(
        ColumnSummary::decode(&bytes[..bytes.len() / 2]),
        Err(SummaryDecodeError::Length)
    );
    bytes[0] = 99;
    assert_eq!(
        ColumnSummary::decode(&bytes),
        Err(SummaryDecodeError::UnknownFormat(99))
    );
}

/// Snapshot clones share their immutable payload instead of allocating another encoding.
#[test]
fn encoding_clones_share_the_payload() {
    let encoded = hilly().encode();
    let cloned = encoded.clone();
    assert!(std::ptr::eq(encoded.bytes.as_ref(), cloned.bytes.as_ref()));
    assert_eq!(ColumnSummary::decode(&cloned.bytes), Ok(hilly()));
}
