//! Prepares a WebM for the client's media profile and writes its signed-bundle descriptor.

use anyhow::{Context, Result, bail, ensure};
use server_experience::{
    crypto,
    media::descriptor::{Descriptor, Profile},
};
use std::collections::BTreeSet;

/// Chunk size of descriptors written here; each chunk is hashed before the demuxer sees it.
pub const CHUNK_BYTES: u32 = 256 * 1024;

/// Largest chunk the client's descriptor validation accepts.
const MAX_CHUNK_BYTES: u32 = 1024 * 1024;
/// Serialized bytes per chunk hash: 64 hex digits, quotes and a comma.
const HASH_JSON_BYTES: u64 = 67;
/// Generous room for the descriptor's other fields.
const FIELD_JSON_BYTES: u64 = 4096;

/// Chunk size for an object of `len` bytes whose descriptor stays within the player's limit.
pub fn chunk_bytes_for(len: u64) -> Result<u32> {
    let budget =
        (server_experience::policy::MAX_MARKER_BYTES as u64 - FIELD_JSON_BYTES) / HASH_JSON_BYTES;
    let mut chunk = CHUNK_BYTES;
    while len.div_ceil(u64::from(chunk)) > budget {
        ensure!(
            chunk < MAX_CHUNK_BYTES,
            "media of {len} bytes needs more chunk hashes than a descriptor holds"
        );
        chunk *= 2;
    }
    Ok(chunk)
}

const SEGMENT: u32 = 0x1853_8067;
const SEEK_HEAD: u32 = 0x114D_9B74;
const SEEK: u32 = 0x4DBB;
const SEEK_ID: u32 = 0x53AB;
const INFO: u32 = 0x1549_A966;
const TIMESTAMP_SCALE: u32 = 0x2A_D7B1;
const DURATION: u32 = 0x4489;
const TRACKS: u32 = 0x1654_AE6B;
const TRACK_ENTRY: u32 = 0xAE;
const TRACK_TYPE: u32 = 0x83;
const CODEC_ID: u32 = 0x86;
const DEFAULT_DURATION: u32 = 0x23_E383;
const VIDEO: u32 = 0xE0;
const PIXEL_WIDTH: u32 = 0xB0;
const PIXEL_HEIGHT: u32 = 0xBA;
const AUDIO: u32 = 0xE1;
const CHANNELS: u32 = 0x9F;
const TAGS: u32 = 0x1254_C367;
const VOID: u8 = 0xEC;

/// One EBML element: its ID, where it starts, and where its payload starts and ends.
#[derive(Clone, Copy, Debug)]
struct Element {
    id: u32,
    start: usize,
    data: usize,
    end: usize,
}

/// Reads the elements in `bytes[from..to]`; an unknown size runs to `to`.
fn children(bytes: &[u8], from: usize, to: usize) -> Result<Vec<Element>> {
    let mut elements = Vec::new();
    let mut at = from;
    while at < to {
        let (id, id_len) = vint(bytes, at, 4, true)?;
        let (size, size_len) = vint(bytes, at + id_len, 8, false)?;
        let data = at + id_len + size_len;
        let unknown = size == (1 << (7 * size_len)) - 1;
        let end = if unknown {
            to
        } else {
            data.checked_add(usize::try_from(size)?)
                .filter(|end| *end <= to)
                .context("EBML element overruns its parent")?
        };
        elements.push(Element {
            id: u32::try_from(id)?,
            start: at,
            data,
            end,
        });
        at = end;
    }
    Ok(elements)
}

/// Reads a variable-length integer; IDs keep their length marker, sizes drop it.
fn vint(bytes: &[u8], at: usize, max: usize, keep_marker: bool) -> Result<(u64, usize)> {
    let first = *bytes.get(at).context("truncated EBML")?;
    let len = first.leading_zeros() as usize + 1;
    ensure!(len <= max, "invalid EBML length at {at}");
    let raw = bytes.get(at..at + len).context("truncated EBML")?;
    let mut value = u64::from(if keep_marker {
        first
    } else {
        first & (0xFF_u16 >> len) as u8
    });
    for byte in &raw[1..] {
        value = value << 8 | u64::from(*byte);
    }
    Ok((value, len))
}

fn uint(bytes: &[u8], element: Element) -> Result<u64> {
    let raw = &bytes[element.data..element.end];
    ensure!(raw.len() <= 8, "EBML integer too long");
    Ok(raw
        .iter()
        .fold(0, |value, byte| value << 8 | u64::from(*byte)))
}

fn float(bytes: &[u8], element: Element) -> Result<f64> {
    let raw = &bytes[element.data..element.end];
    Ok(match raw.len() {
        4 => f64::from(f32::from_be_bytes(raw.try_into()?)),
        8 => f64::from_be_bytes(raw.try_into()?),
        _ => bail!("EBML float of {} bytes", raw.len()),
    })
}

fn find(elements: &[Element], id: u32) -> Option<Element> {
    elements.iter().copied().find(|element| element.id == id)
}

/// Overwrites `bytes[start..end]` with one Void element of the same total size.
fn void(bytes: &mut [u8], start: usize, end: usize) -> Result<()> {
    let total = end - start;
    let size_len = (1..=8)
        .find(|len| {
            total
                .checked_sub(1 + len)
                .is_some_and(|payload| (payload as u64) < (1 << (7 * len)) - 1)
        })
        .context("element too small to void")?;
    let payload = (total - 1 - size_len) as u64;
    bytes[start] = VOID;
    let marked = payload | 1 << (7 * size_len);
    for index in 0..size_len {
        bytes[start + 1 + index] = (marked >> (8 * (size_len - 1 - index))) as u8;
    }
    bytes[start + 1 + size_len..end].fill(0);
    Ok(())
}

/// Voids every Tags element and every SeekHead entry that points at one, keeping all offsets.
pub fn strip_tags(bytes: &mut [u8]) -> Result<usize> {
    let mut voided = 0;
    for segment in children(bytes, 0, bytes.len())?
        .into_iter()
        .filter(|element| element.id == SEGMENT)
    {
        for element in children(bytes, segment.data, segment.end)? {
            if element.id == TAGS {
                void(bytes, element.start, element.end)?;
                voided += 1;
            } else if element.id == SEEK_HEAD {
                for seek in children(bytes, element.data, element.end)? {
                    if seek.id != SEEK {
                        continue;
                    }
                    let fields = children(bytes, seek.data, seek.end)?;
                    if find(&fields, SEEK_ID)
                        .is_some_and(|id| bytes[id.data..id.end] == TAGS.to_be_bytes())
                    {
                        void(bytes, seek.start, seek.end)?;
                        voided += 1;
                    }
                }
            }
        }
    }
    Ok(voided)
}

/// Stream facts the descriptor declares; the client checks them again while decoding.
#[derive(Debug, PartialEq)]
pub struct Facts {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub duration_us: u64,
    pub audio_channels: u8,
}

/// Reads one AV1 video and one Opus audio track's facts and the segment duration.
pub fn facts(bytes: &[u8]) -> Result<Facts> {
    let segment = find(&children(bytes, 0, bytes.len())?, SEGMENT).context("no Segment")?;
    let top = children(bytes, segment.data, segment.end)?;
    let info = find(&top, INFO).context("no Info")?;
    let info = children(bytes, info.data, info.end)?;
    let scale = find(&info, TIMESTAMP_SCALE)
        .map(|element| uint(bytes, element))
        .transpose()?
        .unwrap_or(1_000_000);
    let duration = float(bytes, find(&info, DURATION).context("no Duration")?)?;
    ensure!(duration.is_finite() && duration > 0.0, "invalid Duration");
    let tracks = find(&top, TRACKS).context("no Tracks")?;
    let (mut video, mut audio) = (None, None);
    for entry in children(bytes, tracks.data, tracks.end)?
        .into_iter()
        .filter(|element| element.id == TRACK_ENTRY)
    {
        let fields = children(bytes, entry.data, entry.end)?;
        let codec = find(&fields, CODEC_ID)
            .map(|element| &bytes[element.data..element.end])
            .unwrap_or_default();
        match uint(bytes, find(&fields, TRACK_TYPE).context("no TrackType")?)? {
            1 => {
                ensure!(
                    codec == b"V_AV1" && video.is_none(),
                    "expected one AV1 video track"
                );
                let frame_ns = uint(
                    bytes,
                    find(&fields, DEFAULT_DURATION).context("no video DefaultDuration")?,
                )?;
                ensure!(frame_ns > 0, "invalid DefaultDuration");
                let geometry = find(&fields, VIDEO).context("no Video")?;
                let geometry = children(bytes, geometry.data, geometry.end)?;
                video = Some((
                    uint(
                        bytes,
                        find(&geometry, PIXEL_WIDTH).context("no PixelWidth")?,
                    )?,
                    uint(
                        bytes,
                        find(&geometry, PIXEL_HEIGHT).context("no PixelHeight")?,
                    )?,
                    (1e9 / frame_ns as f64).round(),
                ));
            }
            2 => {
                ensure!(
                    codec == b"A_OPUS" && audio.is_none(),
                    "expected one Opus audio track"
                );
                let format = find(&fields, AUDIO).context("no Audio")?;
                let format = children(bytes, format.data, format.end)?;
                audio = Some(uint(
                    bytes,
                    find(&format, CHANNELS).context("no Channels")?,
                )?);
            }
            other => bail!("unexpected track type {other}"),
        }
    }
    let (width, height, fps) = video.context("no video track")?;
    Ok(Facts {
        width: u32::try_from(width)?,
        height: u32::try_from(height)?,
        fps: fps as u32,
        duration_us: (duration * scale as f64 / 1000.0).ceil() as u64,
        audio_channels: u8::try_from(audio.context("no audio track")?)?,
    })
}

/// Describes stripped WebM `bytes` served at `url`, checked as the client would.
pub fn descriptor(bytes: &[u8], url: &str, id: &str, poster: &str) -> Result<Descriptor> {
    let facts = facts(bytes)?;
    let chunk_bytes = chunk_bytes_for(bytes.len() as u64)?;
    let descriptor = Descriptor {
        id: id.to_owned(),
        timeline: id.to_owned(),
        profile: Profile::WebmAv1OpusBt709,
        url: url.to_owned(),
        bytes: bytes.len() as u64,
        chunk_bytes,
        chunk_hashes: bytes
            .chunks(chunk_bytes as usize)
            .map(crypto::digest)
            .collect(),
        sha256: crypto::digest(bytes),
        width: facts.width,
        height: facts.height,
        fps: facts.fps,
        duration_us: facts.duration_us,
        audio_channels: facts.audio_channels,
        poster: poster.to_owned(),
    };
    ensure!(
        serde_json::to_vec(&descriptor)?.len() <= server_experience::policy::MAX_MARKER_BYTES,
        "the descriptor exceeds the client's size limit"
    );
    let origin = url::Url::parse(url)?.origin().ascii_serialization();
    descriptor
        .validate(&BTreeSet::from([origin]))
        .context("the client would reject this descriptor")?;
    Ok(descriptor)
}
