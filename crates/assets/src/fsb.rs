//! Lenient runtime decoder for single-subsound FSB5 banks (FADPCM and PCM16).
//!
//! Unknown metadata chunks are skipped; only structurally unusable banks fail.

use thiserror::Error;

pub const MAX_FSB_INPUT_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_FSB_PCM_BYTES: usize = 128 * 1024 * 1024;
const BASE_HEADER: usize = 60;
const BLOCK_BYTES: usize = 140;
const BLOCK_FRAMES: usize = 256;
const CODEC_PCM16: u32 = 2;
const CODEC_FADPCM: u32 = 16;
const CHUNK_FREQUENCY: u32 = 2;
const RATES: [u32; 11] = [
    4000, 8000, 11000, 11025, 16000, 22050, 24000, 32000, 44100, 48000, 96000,
];

#[derive(Debug, Error, PartialEq, Eq)]
pub enum FsbError {
    #[error("FSB input or decoded PCM exceeds its bound")]
    TooLarge,
    #[error("unsupported FSB layout: {0}")]
    Unsupported(&'static str),
    #[error("malformed FSB data: {0}")]
    Malformed(&'static str),
}

/// Interleaved PCM16 with its stream format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedSound {
    pub channels: u8,
    pub sample_rate: u32,
    pub samples: Vec<i16>,
}

impl DecodedSound {
    pub fn frames(&self) -> usize {
        self.samples.len() / usize::from(self.channels.max(1))
    }
}

fn word(bytes: &[u8], offset: usize) -> Result<u32, FsbError> {
    bytes
        .get(offset..offset.saturating_add(4))
        .and_then(|slice| slice.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or(FsbError::Malformed("truncated word"))
}

pub fn decode_fsb5(input: &[u8]) -> Result<DecodedSound, FsbError> {
    if input.len() > MAX_FSB_INPUT_BYTES {
        return Err(FsbError::TooLarge);
    }
    if input.len() < BASE_HEADER + 8 || input.get(..4) != Some(b"FSB5") {
        return Err(FsbError::Malformed("magic or base header"));
    }
    if word(input, 8)? != 1 {
        return Err(FsbError::Unsupported("multiple subsounds"));
    }
    let header_len = word(input, 12)? as usize;
    let names_len = word(input, 16)? as usize;
    let data_len = word(input, 20)? as usize;
    let codec = word(input, 24)?;
    let names_start = BASE_HEADER
        .checked_add(header_len)
        .ok_or(FsbError::Malformed("header size"))?;
    let data_start = names_start
        .checked_add(names_len)
        .ok_or(FsbError::Malformed("name size"))?;
    if header_len < 8
        || data_start
            .checked_add(data_len)
            .is_none_or(|end| end > input.len())
    {
        return Err(FsbError::Malformed("section lengths"));
    }
    let header = &input[BASE_HEADER..names_start];
    let mode = u64::from_le_bytes(header[..8].try_into().expect("checked header length"));
    let channels: u8 = match (mode >> 5) & 3 {
        0 => 1,
        1 => 2,
        _ => return Err(FsbError::Unsupported("channel layout")),
    };
    let mut sample_rate = *RATES
        .get(((mode >> 1) & 15) as usize)
        .ok_or(FsbError::Unsupported("rate code"))?;
    let offset = (((mode >> 7) & 0x07ff_ffff) as usize) * 16;
    let frames = ((mode >> 34) & 0x3fff_ffff) as usize;
    let mut more = mode & 1 != 0;
    let mut cursor = 8_usize;
    while more {
        let chunk = word(header, cursor)?;
        cursor += 4;
        let length = ((chunk >> 1) & 0x00ff_ffff) as usize;
        let payload = header
            .get(cursor..cursor.saturating_add(length))
            .ok_or(FsbError::Malformed("chunk length"))?;
        if chunk >> 25 == CHUNK_FREQUENCY && payload.len() == 4 {
            sample_rate = word(payload, 0)?;
        }
        more = chunk & 1 != 0;
        cursor += length;
    }
    if frames == 0 || !(1000..=192_000).contains(&sample_rate) {
        return Err(FsbError::Malformed("frame count or sample rate"));
    }
    let sample_count = frames
        .checked_mul(usize::from(channels))
        .filter(|count| count.saturating_mul(2) <= MAX_FSB_PCM_BYTES)
        .ok_or(FsbError::TooLarge)?;
    let data = input[data_start..data_start + data_len]
        .get(offset..)
        .ok_or(FsbError::Malformed("sample offset"))?;
    let samples = match codec {
        CODEC_PCM16 => {
            let bytes = data
                .get(..sample_count * 2)
                .ok_or(FsbError::Malformed("truncated PCM"))?;
            bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| i16::from_le_bytes(*pair))
                .collect()
        }
        CODEC_FADPCM => decode_fadpcm(data, frames, channels)?,
        _ => return Err(FsbError::Unsupported("codec")),
    };
    Ok(DecodedSound {
        channels,
        sample_rate,
        samples,
    })
}

fn decode_fadpcm(data: &[u8], frames: usize, channels: u8) -> Result<Vec<i16>, FsbError> {
    let channel_count = usize::from(channels);
    let group_bytes = BLOCK_BYTES * channel_count;
    let groups = frames.div_ceil(BLOCK_FRAMES);
    let required = groups
        .checked_mul(group_bytes)
        .ok_or(FsbError::Malformed("block size overflow"))?;
    let data = data
        .get(..required)
        .ok_or(FsbError::Malformed("truncated FADPCM"))?;
    let mut samples = Vec::with_capacity(frames * channel_count);
    for (index, group) in data.chunks_exact(group_bytes).enumerate() {
        let decoded: Vec<[i16; BLOCK_FRAMES]> = group
            .as_chunks::<BLOCK_BYTES>()
            .0
            .iter()
            .map(|block| decode_block(block))
            .collect();
        let count = (frames - index * BLOCK_FRAMES).min(BLOCK_FRAMES);
        for frame in 0..count {
            for channel in &decoded {
                samples.push(channel[frame]);
            }
        }
    }
    Ok(samples)
}

// One fixed-size block: 8 selector nibbles, 8 shift nibbles, two history words, 256 nibbles.
fn decode_block(block: &[u8]) -> [i16; BLOCK_FRAMES] {
    let selectors = u32::from_le_bytes([block[0], block[1], block[2], block[3]]);
    let shifts = u32::from_le_bytes([block[4], block[5], block[6], block[7]]);
    let mut recent = i64::from(i16::from_le_bytes([block[8], block[9]]));
    let mut older = i64::from(i16::from_le_bytes([block[10], block[11]]));
    let mut output = [0_i16; BLOCK_FRAMES];
    for (index, slot) in output.iter_mut().enumerate() {
        let group = index / 32;
        let (first, second) = match ((selectors >> (group * 4)) & 15) % 7 {
            1 => (60, 0),
            2 => (122, 60),
            3 => (115, 52),
            4 => (98, 55),
            _ => (0, 0),
        };
        let shift = (shifts >> (group * 4)) & 15;
        let nibble = i64::from((block[12 + index / 2] >> ((index % 2) * 4)) & 15);
        let signed = if nibble >= 8 { nibble - 16 } else { nibble };
        let value = ((signed * (1_i64 << (6 + shift)) + first * recent - second * older) >> 6)
            .clamp(i64::from(i16::MIN), i64::from(i16::MAX));
        *slot = value as i16;
        older = recent;
        recent = value;
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bank(codec: u32, mode: u64, chunks: &[u8], data: &[u8]) -> Vec<u8> {
        let mut header = mode.to_le_bytes().to_vec();
        header.extend_from_slice(chunks);
        let mut out = b"FSB5".to_vec();
        for value in [1, 1, header.len() as u32, 0, data.len() as u32, codec, 0, 0] {
            out.extend(value.to_le_bytes());
        }
        out.resize(BASE_HEADER, 0);
        out.extend(header);
        out.extend_from_slice(data);
        out
    }

    #[test]
    fn pcm16_mono_round_trips() {
        let data: Vec<u8> = [1_i16, -2, 3]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        // Rate index 9 (48 kHz), one channel, three frames.
        let mode = (9_u64 << 1) | (3_u64 << 34);
        let sound = decode_fsb5(&bank(CODEC_PCM16, mode, &[], &data)).expect("decode");
        assert_eq!((sound.channels, sound.sample_rate), (1, 48_000));
        assert_eq!(sound.samples, [1, -2, 3]);
    }

    #[test]
    fn fadpcm_silence_and_unknown_chunks_are_skipped() {
        // One extra chunk of an unknown type (7) with a 4-byte payload.
        let chunk = ((7_u32 << 25) | (4 << 1)).to_le_bytes();
        let mut chunks = chunk.to_vec();
        chunks.extend([0; 4]);
        let mode = 1_u64 | (8_u64 << 1) | (10_u64 << 34);
        let sound =
            decode_fsb5(&bank(CODEC_FADPCM, mode, &chunks, &[0; BLOCK_BYTES])).expect("decode");
        assert_eq!(sound.frames(), 10);
        assert_eq!(sound.sample_rate, 44_100);
        assert!(sound.samples.iter().all(|sample| *sample == 0));
    }

    #[test]
    fn frequency_chunk_overrides_the_rate_code() {
        let chunk = ((CHUNK_FREQUENCY << 25) | (4 << 1)).to_le_bytes();
        let mut chunks = chunk.to_vec();
        chunks.extend(22_050_u32.to_le_bytes());
        let mode = 1_u64 | (2_u64 << 34);
        let data = [0_u8; 4];
        let sound = decode_fsb5(&bank(CODEC_PCM16, mode, &chunks, &data)).expect("decode");
        assert_eq!(sound.sample_rate, 22_050);
    }

    #[test]
    fn truncated_data_is_rejected_not_guessed() {
        let mode = (9_u64 << 1) | (600_u64 << 34);
        assert!(decode_fsb5(&bank(CODEC_FADPCM, mode, &[], &[0; BLOCK_BYTES])).is_err());
        assert!(decode_fsb5(b"nope").is_err());
    }
}
