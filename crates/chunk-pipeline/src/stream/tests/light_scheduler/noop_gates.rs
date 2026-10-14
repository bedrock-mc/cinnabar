//! Retiring a section whose face light every survivor cell already exceeds supports nothing, so
//! its survivors keep their light; streaming still converges to the light of a single load.

use super::*;

const CAMERA: [f32; 3] = [24.0, 8.0, 24.0];

/// Commits a resident section and marks it changed, as an arriving section is.
fn arrive(stream: &mut WorldStream, key: SubChunkKey, section: SubChunk) {
    stream.authority.commit_sub_chunk(key, section).unwrap();
    stream.resident.insert(key);
    stream.mark_changed(key, Instant::now());
}

/// Retiring a neighbour keeps a survivor's light unless the neighbour could seed a face cell up to
/// that cell's own light.
#[test]
fn eviction_relights_only_survivors_the_retired_face_could_light() {
    for (retired_block, survivor_block, relit) in [(13, 14, false), (0, 0, false), (15, 14, true)] {
        let mut stream = lit_stream(1);
        let retired = SubChunkKey::new(1, 0, 0, 0);
        let survivor = SubChunkKey::new(1, 1, 0, 0);
        for (key, block) in [(retired, retired_block), (survivor, survivor_block)] {
            stream
                .authority
                .commit_sub_chunk(key, super::uniform_sub_chunk(3))
                .unwrap();
            install_current_light(&mut stream, key, block, 0, false);
        }

        stream.evict_column(retired.chunk());

        assert_eq!(
            stream.lighting.jobs.pending.contains_key(&survivor),
            relit,
            "retired {retired_block}, survivor {survivor_block}"
        );
    }
}

/// Air, emitters, opaque and transparent cells in a fixed pseudo-random pattern.
fn patterned_sub_chunk(seed: u32) -> SubChunk {
    let mut bytes = vec![8, 1, (2 << 1) | 1];
    let mut words = [0_u32; 256];
    let mut state = seed.wrapping_mul(0x9e37_79b9) | 1;
    for linear in 0..4096_usize {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        let id = match state % 64 {
            0 => 1,
            1..=24 => 2,
            25..=28 => 3,
            _ => 0,
        };
        words[linear / 16] |= id << ((linear % 16) * 2);
    }
    for word in words {
        bytes.extend(word.to_le_bytes());
    }
    bytes.extend(super::super::zig_zag_i32(4));
    for id in 0..4 {
        bytes.extend(super::super::zig_zag_i32(id));
    }
    SubChunk::decode(&bytes, &world::RawBlockIds { air: 0 })
}

fn section(key: SubChunkKey) -> SubChunk {
    patterned_sub_chunk((key.x * 31 + key.y * 7 + key.z * 131 + 1000) as u32)
}

/// Light streamed in column by column, with the gated relights, and then partly retired,
/// equals the light of loading the surviving columns at once.
#[test]
fn gated_streaming_converges_to_the_light_of_a_single_load() {
    let columns = (0..3)
        .flat_map(|x| (0..3).map(move |z| ChunkKey::new(1, x, z)))
        .collect::<Vec<_>>();
    let keys = |column: ChunkKey| (0..2).map(move |y| SubChunkKey::from_chunk(column, y));
    let retired = [ChunkKey::new(1, 0, 0), ChunkKey::new(1, 0, 1)];

    let mut streamed = lit_stream(1);
    for &column in &columns {
        for key in keys(column) {
            arrive(&mut streamed, key, section(key));
        }
        settle_light(&mut streamed, CAMERA);
    }
    for column in retired {
        streamed.evict_column(column);
        settle_light(&mut streamed, CAMERA);
    }

    let surviving = columns
        .iter()
        .copied()
        .filter(|column| !retired.contains(column))
        .flat_map(keys)
        .collect::<Vec<_>>();
    assert_light_matches_single_load(
        &streamed,
        1,
        surviving.into_iter().map(|key| (key, section(key))),
    );
}

/// Each section's light in `streamed` equals the light of committing `sections` at once.
fn assert_light_matches_single_load(
    streamed: &WorldStream,
    dimension: i32,
    sections: impl IntoIterator<Item = (SubChunkKey, SubChunk)>,
) {
    let mut loaded = lit_stream(dimension);
    let mut keys = Vec::new();
    for (key, section) in sections {
        loaded.authority.commit_sub_chunk(key, section).unwrap();
        loaded.resident.insert(key);
        keys.push(key);
    }
    loaded.mark_changed_sources(keys.iter().copied(), Instant::now());
    settle_light(&mut loaded, CAMERA);
    for key in keys {
        assert!(streamed.light_is_current(key) && loaded.light_is_current(key));
        let (streamed, loaded) = (
            streamed.lighting.store.light(key).unwrap(),
            loaded.lighting.store.light(key).unwrap(),
        );
        for channel in [LightChannel::Block, LightChannel::Sky] {
            for linear in 0..4096_u16 {
                let [x, y, z] = [linear >> 8, linear & 15, (linear >> 4) & 15].map(|v| v as u8);
                assert_eq!(
                    streamed.get(channel, x, y, z),
                    loaded.get(channel, x, y, z),
                    "{key:?} {channel:?} {x},{y},{z}"
                );
            }
        }
    }
}

/// Replacing an emitter while its batch with the section below is in flight leaves no light
/// from the removed emitter in either section.
#[test]
fn replacing_an_emitter_during_its_first_batch_relights_the_section_below() {
    let mut stream = lit_stream(1);
    let lower = SubChunkKey::new(1, 0, 0, 0);
    let upper = SubChunkKey::new(1, 0, 1, 0);
    arrive(&mut stream, lower, super::uniform_sub_chunk(3));
    arrive(&mut stream, upper, super::uniform_sub_chunk(1));
    assert!(stream.dispatch_light_jobs(CAMERA, usize::MAX) > 0);

    stream
        .authority
        .commit_sub_chunk(upper, super::uniform_sub_chunk(2))
        .unwrap();
    stream.mark_changed(upper, Instant::now());
    settle_light(&mut stream, CAMERA);

    assert_light_matches_single_load(
        &stream,
        1,
        [
            (lower, super::uniform_sub_chunk(3)),
            (upper, super::uniform_sub_chunk(2)),
        ],
    );
}

/// An opaque section arriving above a settled sky-lit one moves the column's sky seed plane, so
/// the section below loses its direct sky light.
#[test]
fn a_first_arrival_above_the_column_top_relights_the_section_below() {
    let mut stream = lit_stream(0);
    let lower = SubChunkKey::new(0, 0, 19, 0);
    let upper = SubChunkKey::new(0, 0, 20, 0);
    arrive(&mut stream, lower, super::uniform_sub_chunk(3));
    settle_light(&mut stream, CAMERA);
    let sky_top = |stream: &WorldStream| {
        stream
            .lighting
            .store
            .light(lower)
            .unwrap()
            .get(LightChannel::Sky, 8, 15, 8)
    };
    assert_eq!(sky_top(&stream), Some(15));

    arrive(&mut stream, upper, super::uniform_sub_chunk(2));
    settle_light(&mut stream, CAMERA);

    assert_ne!(sky_top(&stream), Some(15));
    assert_light_matches_single_load(
        &stream,
        0,
        [
            (lower, super::uniform_sub_chunk(3)),
            (upper, super::uniform_sub_chunk(2)),
        ],
    );
}
