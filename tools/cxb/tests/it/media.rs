use cinnabar_cxb::media::{CHUNK_BYTES, Facts, chunk_bytes_for, descriptor, facts, strip_tags};

/// Encodes an element with a one-byte size, enough for these small fixtures.
fn element(id: &[u8], payload: &[u8]) -> Vec<u8> {
    assert!(payload.len() < 127);
    [id, &[0x80 | payload.len() as u8], payload].concat()
}

fn seek(id: &[u8], position: u8) -> Vec<u8> {
    element(
        &[0x4d, 0xbb],
        &[
            element(&[0x53, 0xab], id),
            element(&[0x53, 0xac], &[position]),
        ]
        .concat(),
    )
}

const TAGS: [u8; 4] = [0x12, 0x54, 0xc3, 0x67];
const INFO: [u8; 4] = [0x15, 0x49, 0xa9, 0x66];

/// A header, a seek head naming Info and Tags, Info, two tracks and Tags.
fn webm() -> Vec<u8> {
    let header = element(&[0x1a, 0x45, 0xdf, 0xa3], &element(&[0x42, 0x82], b"webm"));
    let seek_head = element(
        &[0x11, 0x4d, 0x9b, 0x74],
        &[seek(&INFO, 1), seek(&TAGS, 2)].concat(),
    );
    let info = element(
        &INFO,
        &[
            element(&[0x2a, 0xd7, 0xb1], &[0x0f, 0x42, 0x40]),
            element(&[0x44, 0x89], &2500.0f32.to_be_bytes()),
        ]
        .concat(),
    );
    let video = element(
        &[0xae],
        &[
            element(&[0x83], &[1]),
            element(&[0x86], b"V_AV1"),
            element(&[0x23, 0xe3, 0x83], &41_666_667u32.to_be_bytes()),
            element(
                &[0xe0],
                &[
                    element(&[0xb0], &[0x05, 0x00]),
                    element(&[0xba], &[0x02, 0xd0]),
                ]
                .concat(),
            ),
        ]
        .concat(),
    );
    let audio = element(
        &[0xae],
        &[
            element(&[0x83], &[2]),
            element(&[0x86], b"A_OPUS"),
            element(&[0xe1], &element(&[0x9f], &[2])),
        ]
        .concat(),
    );
    let tracks = element(&[0x16, 0x54, 0xae, 0x6b], &[video, audio].concat());
    let tags = element(&TAGS, &element(&[0x73, 0x73], b"encoder"));
    let segment = element(
        &[0x18, 0x53, 0x80, 0x67],
        &[seek_head, info, tracks, tags].concat(),
    );
    [header, segment].concat()
}

#[test]
fn tags_and_their_seek_entry_become_voids_of_equal_size() {
    let original = webm();
    let mut bytes = original.clone();
    assert_eq!(strip_tags(&mut bytes).unwrap(), 2);
    assert_eq!(bytes.len(), original.len());
    let find =
        |haystack: &[u8], needle: &[u8]| haystack.windows(needle.len()).position(|w| w == needle);
    assert!(find(&bytes, &TAGS).is_none(), "a Tags ID survived");
    assert!(find(&bytes, b"encoder").is_none());
    // The Info entry and every byte outside the voided ranges are untouched.
    let info = find(&original, &[0x53, 0xab, 0x84, 0x15]).unwrap();
    assert_eq!(bytes[info..info + 8], original[info..info + 8]);
    let tags = original.windows(4).rposition(|w| w == TAGS).unwrap();
    assert_eq!(bytes[tags], 0xec);
    assert_eq!(facts(&bytes).unwrap(), facts(&original).unwrap());
    assert_eq!(strip_tags(&mut bytes).unwrap(), 0);
}

#[test]
fn facts_come_from_tracks_and_scaled_duration() {
    assert_eq!(
        facts(&webm()).unwrap(),
        Facts {
            width: 1280,
            height: 720,
            fps: 24,
            duration_us: 2_500_000,
            audio_channels: 2,
        }
    );
}

#[test]
fn descriptor_indexes_every_chunk_and_passes_client_validation() {
    let mut bytes = webm();
    strip_tags(&mut bytes).unwrap();
    // A trailing top-level Void pushes the object past one chunk, so a short final chunk exists.
    let pad = CHUNK_BYTES as u64;
    bytes.push(0xec);
    bytes.extend_from_slice(&(pad | 1 << 56).to_be_bytes());
    bytes.resize(bytes.len() + pad as usize, 0);
    let described = descriptor(
        &bytes,
        "https://media.example/intro.webm",
        "cinema.clip",
        "media/clip.png",
    )
    .unwrap();
    assert_eq!(described.chunk_hashes.len(), 2);
    assert_eq!(
        described.chunk_hashes[1],
        server_experience::crypto::digest(&bytes[CHUNK_BYTES as usize..])
    );
    assert_eq!(described.bytes, bytes.len() as u64);
    assert_eq!(described.timeline, "cinema.clip");
    let json = serde_json::to_vec(&described).unwrap();
    let back: server_experience::media::descriptor::Descriptor =
        serde_json::from_slice(&json).unwrap();
    assert_eq!(back.sha256, described.sha256);
    assert!(
        descriptor(
            &bytes,
            "http://media.example/intro.webm",
            "cinema.clip",
            "p.png"
        )
        .is_err()
    );
    let mut vp9 = webm();
    let video = vp9.windows(5).position(|w| w == b"V_AV1").unwrap();
    vp9[video..video + 5].copy_from_slice(b"V_VP9");
    assert!(
        descriptor(
            &vp9,
            "https://media.example/intro.webm",
            "cinema.clip",
            "p.png"
        )
        .is_err()
    );
}

#[test]
fn large_media_gets_chunks_that_keep_its_descriptor_within_the_player_limit() {
    let len = 256 * 1024 * 1024;
    let chunk = chunk_bytes_for(len).unwrap();
    let hashes = len.div_ceil(u64::from(chunk));
    let descriptor = server_experience::media::descriptor::Descriptor {
        id: "cinema.clip".into(),
        timeline: "cinema.clip".into(),
        profile: server_experience::media::descriptor::Profile::WebmAv1OpusBt709,
        url: "https://media.example/clip.webm".into(),
        bytes: len,
        chunk_bytes: chunk,
        chunk_hashes: vec![server_experience::crypto::digest(b""); hashes as usize],
        sha256: server_experience::crypto::digest(b""),
        width: 1280,
        height: 720,
        fps: 30,
        duration_us: 1_000_000,
        audio_channels: 2,
        poster: "media/clip.png".into(),
    };
    let json = serde_json::to_vec(&descriptor).unwrap();
    assert!(
        json.len() <= server_experience::policy::MAX_MARKER_BYTES,
        "{} bytes",
        json.len()
    );
    assert_eq!(chunk_bytes_for(4 * 1024 * 1024).unwrap(), CHUNK_BYTES);
    assert!(chunk_bytes_for(8 * 1024 * 1024 * 1024).is_err());
}
