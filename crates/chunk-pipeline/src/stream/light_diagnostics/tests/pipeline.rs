use super::*;
use client_world::ingestion::{
    HeightmapDiagnostic, SubChunkBatchEvent, SubChunkResult, SubChunkUnavailable, dimension_slots,
    prepare_sub_chunks,
};

/// Commits decoded column fixtures through the same prepared-event route as network workers.
fn column(stream: &mut WorldStream, mode: LevelChunkMode) {
    let range = vanilla_dimension_range(0).unwrap();
    let key = ChunkKey::new(0, 0, 0);
    let ids = stream.decode_ids(0);
    let event = LevelChunkEvent {
        dimension: 0,
        x: 0,
        z: 0,
        mode,
        payload: vec![],
    };
    let prepared = match mode {
        LevelChunkMode::Inline { count } => {
            let payload = if count == 0 {
                vec![]
            } else {
                vec![9, 1, range.base_sub_chunk_y as i8 as u8, 1, 2]
            };
            PreparedWorldEvent::InlineLevelChunk {
                event,
                decoded: world::DecodedLevelChunk::decode_inline(
                    key,
                    dimension_slots(range),
                    count,
                    &payload,
                    &world::RawBlockIds { air: 0 },
                    &ids,
                ),
                duration: Duration::ZERO,
            }
        }
        _ => PreparedWorldEvent::RequestLevelChunk {
            event,
            decoded: world::decode_column_tail(key, dimension_slots(range), &[], &ids),
            duration: Duration::ZERO,
        },
    };
    stream.apply_prepared(prepared);
}

/// Preserves metadata through worker preparation before admitting a response to the stream.
fn reply(
    stream: &mut WorldStream,
    y: i32,
    result: SubChunkResult,
    metadata: Option<SubChunkDiagnostic>,
) {
    let batch = SubChunkBatchEvent {
        dimension: 0,
        entries: vec![protocol::SubChunkEntryEvent {
            position: [0, y, 0],
            result,
            diagnostics: metadata,
        }],
    };
    stream.apply_prepared(PreparedWorldEvent::SubChunks {
        dimension: 0,
        entries: prepare_sub_chunks(batch, &stream.decode_ids(0)),
        duration: Duration::ZERO,
    });
}

#[test]
fn request_errors_and_late_replies_do_not_replace_inline_resident_provenance() {
    let mut stream = stream();
    let key = ChunkKey::new(0, 0, 0);
    let base = vanilla_dimension_range(0).unwrap().base_sub_chunk_y;
    column(&mut stream, LevelChunkMode::Inline { count: 1 });
    assert!(
        stream
            .authority
            .terrain()
            .sub_chunk(SubChunkKey::from_chunk(key, base))
            .is_some()
    );
    for reason in [
        SubChunkUnavailable::Undefined,
        SubChunkUnavailable::YIndexOutOfBounds,
    ] {
        column(&mut stream, LevelChunkMode::LimitedRequests { highest: 1 });
        reply(&mut stream, base, SubChunkResult::Unavailable(reason), None);
        let record = &stream.light_diagnostics.columns[&key].sections[&base];
        assert_eq!(record.source, Some("inline-chunk"));
        assert_eq!(
            record.reply.as_deref(),
            Some(format!("{reason:?}").as_str())
        );
        assert!(
            stream
                .authority
                .terrain()
                .sub_chunk(SubChunkKey::from_chunk(key, base))
                .is_some()
        );
    }
    reply(&mut stream, base, SubChunkResult::AllAir, None);
    let record = &stream.light_diagnostics.columns[&key].sections[&base];
    assert_eq!(record.source, Some("inline-chunk"));
    assert_eq!(record.reply.as_deref(), Some("YIndexOutOfBounds"));
    assert_eq!(stream.stats.phase2_outcomes.stale, 1);
}

#[test]
fn out_of_bounds_reply_preserves_the_source_of_preexisting_known_air() {
    let mut stream = stream();
    let key = ChunkKey::new(0, 0, 0);
    let base = vanilla_dimension_range(0).unwrap().base_sub_chunk_y;
    column(&mut stream, LevelChunkMode::Inline { count: 0 });
    column(&mut stream, LevelChunkMode::LimitedRequests { highest: 1 });
    reply(
        &mut stream,
        base,
        SubChunkResult::Unavailable(SubChunkUnavailable::YIndexOutOfBounds),
        None,
    );
    let sections = &stream.light_diagnostics.columns[&key].sections;
    assert_eq!(sections[&base].source, Some("inline-omitted-air"));
    assert_eq!(sections[&base].reply.as_deref(), Some("YIndexOutOfBounds"));
    assert_eq!(
        sections[&(base + 1)].source,
        Some("limited-request-upper-air")
    );
    assert!(
        stream
            .known_air
            .contains(&SubChunkKey::from_chunk(key, base))
    );
}

#[test]
fn all_air_response_retains_heightmaps_and_limited_header_replaces_upper_evidence() {
    let mut stream = stream();
    let key = ChunkKey::new(0, 0, 0);
    let base = vanilla_dimension_range(0).unwrap().base_sub_chunk_y;
    let metadata = SubChunkDiagnostic {
        payload_present: false,
        heightmap: HeightmapDiagnostic {
            kind: 1,
            payload_present: true,
            sample_count: 256,
            min: Some(-1),
            max: Some(15),
        },
        render_heightmap: HeightmapDiagnostic {
            kind: 4,
            payload_present: false,
            sample_count: 0,
            min: None,
            max: None,
        },
    };
    column(&mut stream, LevelChunkMode::LimitlessRequests);
    assert!(stream.light_diagnostics.columns[&key].sections.is_empty());
    reply(
        &mut stream,
        base + 1,
        SubChunkResult::AllAir,
        Some(metadata),
    );
    let record = &stream.light_diagnostics.columns[&key].sections[&(base + 1)];
    assert_eq!(record.source, Some("request-all-air"));
    assert_eq!(record.reply.as_deref(), Some("SuccessAllAir"));
    assert_eq!(record.metadata, Some(metadata));
    column(&mut stream, LevelChunkMode::LimitedRequests { highest: 1 });
    let record = &stream.light_diagnostics.columns[&key].sections[&(base + 1)];
    assert_eq!(record.source, Some("limited-request-upper-air"));
    assert_eq!(record.reply, None);
    assert_eq!(record.metadata, None);
    reply(
        &mut stream,
        base + 1,
        SubChunkResult::Unavailable(SubChunkUnavailable::Undefined),
        Some(metadata),
    );
    let record = &stream.light_diagnostics.columns[&key].sections[&(base + 1)];
    assert_eq!(record.source, Some("limited-request-upper-air"));
    assert_eq!(record.reply, None);
    assert_eq!(record.metadata, None);
    assert_eq!(stream.stats.phase2_outcomes.stale, 1);
}
