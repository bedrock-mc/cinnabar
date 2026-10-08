//! Ogg Vorbis decoding into the shared PCM16 form, plus format sniffing for sound files.

use std::io::Cursor;

use lewton::inside_ogg::OggStreamReader;

use crate::fsb::{DecodedSound, FsbError, MAX_FSB_INPUT_BYTES, MAX_FSB_PCM_BYTES, decode_fsb5};

/// Decodes mono or stereo Ogg Vorbis; other channel layouts and oversized output are rejected.
pub fn decode_ogg(input: &[u8]) -> Result<DecodedSound, FsbError> {
    if input.len() > MAX_FSB_INPUT_BYTES {
        return Err(FsbError::TooLarge);
    }
    let mut reader = OggStreamReader::new(Cursor::new(input))
        .map_err(|_| FsbError::Malformed("vorbis headers"))?;
    let channels = reader.ident_hdr.audio_channels;
    let sample_rate = reader.ident_hdr.audio_sample_rate;
    if !(1..=2).contains(&channels) || !(1000..=192_000).contains(&sample_rate) {
        return Err(FsbError::Unsupported("vorbis channel layout or rate"));
    }
    let mut samples: Vec<i16> = Vec::new();
    loop {
        match reader.read_dec_packet_itl() {
            Ok(Some(packet)) => {
                if reader.ident_hdr.audio_channels != channels
                    || reader.ident_hdr.audio_sample_rate != sample_rate
                {
                    return Err(FsbError::Unsupported("chained vorbis format change"));
                }
                if (samples.len() + packet.len()) * 2 > MAX_FSB_PCM_BYTES {
                    return Err(FsbError::TooLarge);
                }
                samples.extend(packet);
            }
            Ok(None) => break,
            // A damaged tail keeps the audio decoded so far.
            Err(_) if !samples.is_empty() => break,
            Err(_) => return Err(FsbError::Malformed("vorbis packet")),
        }
    }
    if samples.is_empty() {
        return Err(FsbError::Malformed("empty vorbis stream"));
    }
    Ok(DecodedSound {
        channels,
        sample_rate,
        samples,
    })
}

/// Decodes an FSB5 or Ogg Vorbis sound file, chosen by its magic.
pub fn decode_sound(input: &[u8]) -> Result<DecodedSound, FsbError> {
    match input.get(..4) {
        Some(b"OggS") => decode_ogg(input),
        _ => decode_fsb5(input),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_chained_ogg_rejects_format_changes() {
        assert!(
            decode_ogg(include_bytes!(
                "../tests/fixtures/chained-format-change.ogg"
            ))
            .is_err()
        );
    }

    #[test]
    fn garbage_and_truncated_ogg_are_rejected() {
        assert!(decode_ogg(b"OggS-not-really").is_err());
        assert!(decode_sound(b"OggS").is_err());
        assert!(decode_sound(b"????").is_err());
    }
}
