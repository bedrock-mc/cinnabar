//! Bounded, single-subsound FSB5 version-1 FADPCM decoding.
//!
//! Format references: vgmstream revision 95cff213b1b1fbd292c313270cc679f02a1e624d,
//! `src/meta/fsb5.c` and `src/coding/fadpcm_decoder.c`. This implementation is
//! independently authored. Limits below are compiler safety ceilings, not format
//! constants. Unsupported codecs, chunks, channel layouts and flags fail closed.
//! This module does not select sounds, interpret loop names or activate playback.

const MAX_INPUT_BYTES: usize = 2 * 1024 * 1024;
const MAX_PCM_BYTES: usize = 4 * 1024 * 1024;
const MAX_HEADER_BYTES: usize = 128;
const MAX_NAME_BYTES: usize = 4096;
const MAX_CHUNKS: usize = 8;
const BLOCK_BYTES: usize = 140;
const BLOCK_FRAMES: usize = 256;
const RATES: [u32; 11] = [
    4000, 8000, 11000, 11025, 16000, 22050, 24000, 32000, 44100, 48000, 96000,
];

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FadpcmDecodeError {
    #[error("FSB input exceeds the decoder safety limit")]
    InputTooLarge,
    #[error("decoded PCM exceeds the decoder safety limit")]
    DecodedTooLarge,
    #[error("unsupported FSB subset: {0}")]
    Unsupported(&'static str),
    #[error("malformed FSB data: {0}")]
    Malformed(&'static str),
    #[error("could not allocate bounded decoded PCM")]
    AllocationFailed,
}

/// Validated interleaved PCM16, with no unchecked public construction path.
#[derive(Debug, PartialEq, Eq)]
pub struct DecodedFadpcm {
    channels: u8,
    sample_rate: u32,
    samples: Box<[i16]>,
}

impl DecodedFadpcm {
    pub fn channels(&self) -> u8 {
        self.channels
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn frames(&self) -> usize {
        self.samples.len() / usize::from(self.channels)
    }

    pub fn samples(&self) -> &[i16] {
        &self.samples
    }
}

fn malformed(detail: &'static str) -> FadpcmDecodeError {
    FadpcmDecodeError::Malformed(detail)
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, FadpcmDecodeError> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| malformed("word offset"))?;
    let word = bytes
        .get(offset..end)
        .ok_or_else(|| malformed("truncated word"))?;
    Ok(u32::from_le_bytes(
        word.try_into().map_err(|_| malformed("word width"))?,
    ))
}

fn checked_sum(parts: &[usize]) -> Result<usize, FadpcmDecodeError> {
    parts.iter().try_fold(0_usize, |sum, part| {
        sum.checked_add(*part)
            .ok_or_else(|| malformed("section size overflow"))
    })
}

fn validate_names(names: &[u8]) -> Result<(), FadpcmDecodeError> {
    if names.is_empty() {
        return Ok(());
    }
    let start = usize::try_from(u32_at(names, 0)?).map_err(|_| malformed("name offset"))?;
    if start < 4 || start >= names.len() || names[4..start].iter().any(|byte| *byte != 0) {
        return Err(malformed("name offset or padding"));
    }
    let tail = &names[start..];
    let length = tail
        .iter()
        .position(|byte| *byte == 0)
        .ok_or_else(|| malformed("unterminated name"))?;
    if length > 256 || tail[length..].iter().any(|byte| *byte != 0) {
        return Err(malformed("name length or padding"));
    }
    Ok(())
}

/// Decode only the explicitly supported fixed-codec subset, never truncated or
/// guessed audio. The final frame is decoded completely, then trimmed to the
/// declared per-channel sample count. Padding is not interpreted as audio.
pub fn decode_fsb5_fadpcm(input: &[u8]) -> Result<DecodedFadpcm, FadpcmDecodeError> {
    if input.len() > MAX_INPUT_BYTES {
        return Err(FadpcmDecodeError::InputTooLarge);
    }
    if input.len() < 60 || input.get(..4) != Some(b"FSB5") {
        return Err(malformed("magic or base header"));
    }
    if u32_at(input, 4)? != 1 || u32_at(input, 8)? != 1 || u32_at(input, 24)? != 16 {
        return Err(FadpcmDecodeError::Unsupported(
            "version, subsound count or codec",
        ));
    }
    if u32_at(input, 28)? != 0 || u32_at(input, 32)? != 0 {
        return Err(FadpcmDecodeError::Unsupported("header flags"));
    }
    let header_len = usize::try_from(u32_at(input, 12)?).map_err(|_| malformed("header size"))?;
    let names_len = usize::try_from(u32_at(input, 16)?).map_err(|_| malformed("name size"))?;
    let data_len = usize::try_from(u32_at(input, 20)?).map_err(|_| malformed("data size"))?;
    if !(8..=MAX_HEADER_BYTES).contains(&header_len) || names_len > MAX_NAME_BYTES {
        return Err(malformed("metadata bounds"));
    }
    let names_start = checked_sum(&[60, header_len])?;
    let data_start = checked_sum(&[names_start, names_len])?;
    if checked_sum(&[data_start, data_len])? != input.len() {
        return Err(malformed("section lengths"));
    }
    let header = &input[60..names_start];
    let mode = u64::from_le_bytes(
        header[..8]
            .try_into()
            .map_err(|_| malformed("sample header"))?,
    );
    if ((mode >> 7) & 0x07ff_ffff) != 0 {
        return Err(FadpcmDecodeError::Unsupported("nonzero sample offset"));
    }
    let channels = match (mode >> 5) & 3 {
        0 => 1_u8,
        1 => 2,
        _ => return Err(FadpcmDecodeError::Unsupported("channel layout")),
    };
    let rate_index = ((mode >> 1) & 15) as usize;
    let mut sample_rate = *RATES
        .get(rate_index)
        .ok_or(FadpcmDecodeError::Unsupported("rate code"))?;
    let mut more = mode & 1 != 0;
    let mut cursor = 8_usize;
    let mut chunks = 0;
    let mut rate_override = false;
    let mut loop_range = None;
    while more {
        chunks += 1;
        if chunks > MAX_CHUNKS {
            return Err(malformed("chunk count"));
        }
        let chunk = u32_at(header, cursor)?;
        cursor = checked_sum(&[cursor, 4])?;
        let length = ((chunk >> 1) & 0x00ff_ffff) as usize;
        let end = checked_sum(&[cursor, length])?;
        let payload = header
            .get(cursor..end)
            .ok_or_else(|| malformed("chunk length"))?;
        match chunk >> 25 {
            2 => {
                if rate_override || payload.len() != 4 {
                    return Err(malformed("rate override"));
                }
                sample_rate = u32_at(payload, 0)?;
                rate_override = true;
            }
            // FSB5 type 3 carries two little-endian sample positions, with an
            // inclusive end (vgmstream's pinned fsb5.c parse_header case 0x03).
            // Validate this metadata but keep this decoder finite: playback
            // loop requests are a separate client policy, never activated by
            // metadata commonly present even on short combat one-shot samples.
            3 => {
                if loop_range.is_some() || payload.len() != 8 {
                    return Err(malformed("loop range"));
                }
                loop_range = Some((u32_at(payload, 0)?, u32_at(payload, 4)?));
            }
            _ => return Err(FadpcmDecodeError::Unsupported("metadata chunk")),
        }
        more = chunk & 1 != 0;
        cursor = end;
    }
    if cursor != header.len() || !(4000..=96_000).contains(&sample_rate) {
        return Err(malformed("sample metadata or rate"));
    }
    validate_names(&input[names_start..data_start])?;
    let frames =
        usize::try_from((mode >> 34) & 0x3fff_ffff).map_err(|_| malformed("frame count"))?;
    if loop_range.is_some_and(|(start, end)| start > end || u64::from(end) >= frames as u64) {
        return Err(malformed("loop sample bounds"));
    }
    if frames == 0 {
        return Err(malformed("zero sample count"));
    }
    let sample_count = frames
        .checked_mul(usize::from(channels))
        .ok_or(FadpcmDecodeError::DecodedTooLarge)?;
    if sample_count
        .checked_mul(2)
        .is_none_or(|size| size > MAX_PCM_BYTES)
    {
        return Err(FadpcmDecodeError::DecodedTooLarge);
    }
    let groups = frames.div_ceil(BLOCK_FRAMES);
    let group_bytes = BLOCK_BYTES * usize::from(channels);
    let required = groups
        .checked_mul(group_bytes)
        .ok_or_else(|| malformed("block size overflow"))?;
    let aligned = required
        .checked_add(31)
        .ok_or_else(|| malformed("alignment overflow"))?
        & !31;
    let data = &input[data_start..];
    if (data.len() != required && data.len() != aligned)
        || data.len() < required
        || data[required..].iter().any(|byte| *byte != 0)
    {
        return Err(malformed("compressed size or alignment padding"));
    }
    let mut samples = Vec::new();
    samples
        .try_reserve_exact(sample_count)
        .map_err(|_| FadpcmDecodeError::AllocationFailed)?;
    for (group_index, group) in data[..required].chunks_exact(group_bytes).enumerate() {
        let left = decode_block(&group[..BLOCK_BYTES]);
        let right = if channels == 2 {
            Some(decode_block(&group[BLOCK_BYTES..]))
        } else {
            None
        };
        let count = (frames - group_index * BLOCK_FRAMES).min(BLOCK_FRAMES);
        for index in 0..count {
            samples.push(left[index]);
            if let Some(right) = &right {
                samples.push(right[index]);
            }
        }
    }
    Ok(DecodedFadpcm {
        channels,
        sample_rate,
        samples: samples.into_boxed_slice(),
    })
}

// Private caller guarantees one complete fixed-size block. Coefficients and
// signed nibbles are format arithmetic, not a copied decoder implementation.
fn decode_block(block: &[u8]) -> [i16; BLOCK_FRAMES] {
    let selectors = u32::from_le_bytes([block[0], block[1], block[2], block[3]]);
    let shifts = u32::from_le_bytes([block[4], block[5], block[6], block[7]]);
    let mut recent = i64::from(i16::from_le_bytes([block[8], block[9]]));
    let mut older = i64::from(i16::from_le_bytes([block[10], block[11]]));
    let mut output = [0_i16; BLOCK_FRAMES];
    for (index, output_sample) in output.iter_mut().enumerate() {
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
        *output_sample = value as i16;
        older = recent;
        recent = value;
    }
    output
}

#[cfg(test)]
mod tests;
