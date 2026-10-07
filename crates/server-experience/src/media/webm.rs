//! Developer-only AV1/Opus decode, run only inside the memory-limited helper process.

use super::{
    ceiling::Contained,
    descriptor::Descriptor,
    faults::FaultReader,
    frames::{PcmBlock, VideoFrame, bt709_rgba},
    service::output::Output,
    *,
};
use anyhow::{Result, ensure};
use dav1d::{Decoder, PixelLayout, PlanarImageComponent, Settings};
use matroska_demuxer::{FlagInterlaced, Frame, MatroskaFile, TrackType};
use std::io::{Read, Seek};

const CICP_BT709: u8 = 1;
/// Decoder output carries no generation; the parent process assigns its own.
const HELPER_GENERATION: u64 = 0;

/// Decodes one constrained rendition from the start, emitting output from `start_us` on.
pub fn decode<R: Read + Seek>(
    reader: R,
    descriptor: &Descriptor,
    start_us: u64,
    _contained: &Contained,
    mut emit: impl FnMut(Output) -> Result<()>,
) -> Result<()> {
    let generation = HELPER_GENERATION;
    let (reader, faults) = FaultReader::new(reader);
    let mut file = MatroskaFile::open(reader)?;
    faults.check()?;
    ensure!(
        file.ebml_header().doc_type() == "webm" && file.tracks().len() == 2,
        "unsupported WebM structure"
    );
    ensure!(
        file.chapters().is_none() && file.tags().is_none(),
        "chapters and tags are outside the media profile"
    );
    let video = file
        .tracks()
        .iter()
        .find(|track| track.track_type() == TrackType::Video)
        .ok_or_else(|| anyhow::anyhow!("missing video track"))?;
    let audio = file
        .tracks()
        .iter()
        .find(|track| track.track_type() == TrackType::Audio)
        .ok_or_else(|| anyhow::anyhow!("missing audio track"))?;
    ensure!(
        video.codec_id() == "V_AV1" && audio.codec_id() == "A_OPUS",
        "unsupported codec"
    );
    ensure!(
        file.tracks()
            .iter()
            .all(|track| track.content_encodings().is_none() && !track.flag_lacing()),
        "track transformations or lacing denied"
    );
    let geometry = video
        .video()
        .ok_or_else(|| anyhow::anyhow!("missing video metadata"))?;
    ensure!(
        geometry.pixel_width().get() == u64::from(descriptor.width)
            && geometry.pixel_height().get() == u64::from(descriptor.height)
            && geometry.flag_interlaced() == FlagInterlaced::Progressive
            && geometry.alpha_mode().unwrap_or(0) == 0,
        "video profile mismatch"
    );
    let format = audio
        .audio()
        .ok_or_else(|| anyhow::anyhow!("missing audio metadata"))?;
    ensure!(
        format.channels().get() == u64::from(descriptor.audio_channels)
            && format.sampling_frequency() == f64::from(SAMPLE_RATE),
        "audio profile mismatch"
    );
    let header = audio
        .codec_private()
        .ok_or_else(|| anyhow::anyhow!("missing OpusHead"))?;
    ensure!(
        header.len() == 19
            && &header[..8] == b"OpusHead"
            && header[8] == 1
            && header[9] == descriptor.audio_channels
            && header[18] == 0,
        "unsupported OpusHead"
    );
    let mut skip = usize::from(u16::from_le_bytes([header[10], header[11]]));
    ensure!(skip <= SAMPLE_RATE as usize, "Opus pre-skip too large");
    let delay_ns = audio.codec_delay().unwrap_or(0);
    let expected_delay = skip as u64 * 1_000_000_000 / u64::from(SAMPLE_RATE);
    ensure!(
        delay_ns.abs_diff(expected_delay) <= 1,
        "Opus delay mismatch"
    );
    let video_track = video.track_number().get();
    let audio_track = audio.track_number().get();
    let scale = file.info().timestamp_scale().get();
    let mut settings = Settings::new();
    settings.set_n_threads(4);
    settings.set_max_frame_delay(1);
    settings.set_frame_size_limit(MAX_WIDTH * MAX_HEIGHT);
    settings.set_strict_std_compliance(true);
    let mut av1 = Decoder::with_settings(&settings)?;
    let channels = if descriptor.audio_channels == 1 {
        opus::Channels::Mono
    } else {
        opus::Channels::Stereo
    };
    let mut opus = opus::Decoder::new(SAMPLE_RATE, channels)?;
    opus.set_gain(i32::from(i16::from_le_bytes([header[16], header[17]])))?;
    let mut frame = Frame::default();
    let mut last_video_us = None;
    while file.next_frame(&mut frame)? {
        faults.check()?;
        ensure!(
            frame.data.len() <= MAX_SAMPLE_BYTES,
            "compressed sample too large"
        );
        let pts_us = frame
            .timestamp
            .checked_mul(scale)
            .ok_or_else(|| anyhow::anyhow!("timestamp overflow"))?
            / 1000;
        ensure!(
            pts_us <= descriptor.duration_us.saturating_add(1_000_000),
            "sample exceeds duration"
        );
        if frame.track == video_track {
            if let Some(last) = last_video_us {
                ensure!(
                    pts_us > last
                        && pts_us - last >= (1_000_000 / u64::from(MAX_FPS)).saturating_sub(1000),
                    "decoded frame rate exceeded"
                );
            }
            last_video_us = Some(pts_us);
            let mut result = av1.send_data(
                std::mem::take(&mut frame.data),
                None,
                Some(i64::try_from(pts_us)?),
                None,
            );
            for _ in 0..64 {
                match result {
                    Ok(()) => break,
                    Err(dav1d::Error::Again) => {
                        drain(&mut av1, descriptor, start_us, &mut emit)?;
                        result = av1.send_pending_data();
                    }
                    Err(error) => return Err(error.into()),
                }
            }
            result?;
            drain(&mut av1, descriptor, start_us, &mut emit)?;
        } else if frame.track == audio_track {
            let channels = usize::from(descriptor.audio_channels);
            let mut samples = vec![0.0; OPUS_PACKET_FRAMES * channels];
            let count = opus.decode_float(&frame.data, &mut samples, false)?;
            ensure!(count <= OPUS_PACKET_FRAMES, "Opus packet duration exceeded");
            samples.truncate(count * channels);
            clamp_opus(&mut samples)?;
            let skipped = skip.min(count);
            skip -= skipped;
            samples.drain(..skipped * channels);
            let corrected = i128::from(pts_us) - i128::from(delay_ns / 1000)
                + (skipped as i128 * 1_000_000 / i128::from(SAMPLE_RATE));
            let pts_us = u64::try_from(corrected.max(0))?;
            if !samples.is_empty() && pts_us >= start_us {
                let block = PcmBlock {
                    generation,
                    pts_us,
                    channels: descriptor.audio_channels,
                    samples,
                };
                block.validate(generation)?;
                emit(Output::Audio(block))?;
            }
        } else {
            anyhow::bail!("unexpected track");
        }
    }
    faults.check()?;
    drain(&mut av1, descriptor, start_us, &mut emit)?;
    emit(Output::End)
}

/// Copies validated dav1d planes into one bounded frame; frames before `start_us` skip conversion.
fn drain(
    decoder: &mut Decoder,
    descriptor: &Descriptor,
    start_us: u64,
    emit: &mut impl FnMut(Output) -> Result<()>,
) -> Result<()> {
    for _ in 0..64 {
        let picture = match decoder.get_picture() {
            Ok(picture) => picture,
            Err(dav1d::Error::Again) => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        ensure!(
            picture.width() == descriptor.width
                && picture.height() == descriptor.height
                && picture.bit_depth() == 8
                && picture.pixel_layout() == PixelLayout::I420,
            "decoded profile changed"
        );
        ensure!(
            picture.matrix_coefficients() as u8 == CICP_BT709
                && picture.color_primaries() as u8 == CICP_BT709
                && picture.transfer_characteristic() as u8 == CICP_BT709
                && picture.color_range() == dav1d::pixel::YUVRange::Limited,
            "unsupported decoded color profile"
        );
        let pts_us = u64::try_from(
            picture
                .timestamp()
                .ok_or_else(|| anyhow::anyhow!("missing video PTS"))?,
        )?;
        if pts_us < start_us {
            continue;
        }
        let components = [
            PlanarImageComponent::Y,
            PlanarImageComponent::U,
            PlanarImageComponent::V,
        ];
        let planes = components.map(|component| picture.plane(component));
        let strides = components.map(|component| picture.stride(component) as usize);
        let rgba = bt709_rgba(
            picture.width(),
            picture.height(),
            [&planes[0], &planes[1], &planes[2]],
            strides,
        )?;
        let frame = VideoFrame {
            generation: HELPER_GENERATION,
            pts_us,
            width: picture.width(),
            height: picture.height(),
            rgba,
        };
        frame.validate(HELPER_GENERATION)?;
        emit(Output::Video(frame))?;
    }
    anyhow::bail!("too many decoded frames in one dispatch")
}

/// Preserves finite decoder headroom and positive OpusHead gain without rejecting loud audio.
fn clamp_opus(samples: &mut [f32]) -> Result<()> {
    for sample in samples {
        ensure!(sample.is_finite(), "nonfinite Opus output");
        *sample = sample.clamp(-1.0, 1.0);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &[u8] = include_bytes!("testdata/fixture.webm");

    /// Decodes the committed 64x64, 10 fps, half-second AV1 + mono Opus test pattern.
    fn decode_fixture(start_us: u64) -> Vec<Output> {
        let descriptor = super::super::ranges::tests::descriptor_for(FIXTURE);
        descriptor.validate_profile().unwrap();
        let reader = super::super::ranges::RangeReader::new(
            descriptor.clone(),
            super::super::ranges::tests::MemoryChunks {
                bytes: FIXTURE.to_vec(),
                corrupt: None,
                loads: 0,
            },
        );
        let mut outputs = Vec::new();
        decode(reader, &descriptor, start_us, &Contained(()), |output| {
            outputs.push(output);
            Ok(())
        })
        .unwrap();
        outputs
    }

    #[test]
    fn fixture_decodes_to_ordered_video_and_audio_then_end() {
        let outputs = decode_fixture(0);
        let frames: Vec<_> = outputs
            .iter()
            .filter_map(|output| match output {
                Output::Video(frame) => Some(frame),
                _ => None,
            })
            .collect();
        assert_eq!(frames.len(), 5);
        assert!(
            frames
                .windows(2)
                .all(|pair| pair[0].pts_us < pair[1].pts_us)
        );
        assert!(
            frames
                .iter()
                .all(|frame| frame.width == 64 && frame.rgba.len() == 64 * 64 * 4)
        );
        assert!(
            frames[0]
                .rgba
                .chunks(4)
                .any(|pixel| pixel[..3] != [0, 0, 0])
        );
        let audio: usize = outputs
            .iter()
            .filter_map(|output| match output {
                Output::Audio(block) => Some(block.samples.len()),
                _ => None,
            })
            .sum();
        assert!(audio > SAMPLE_RATE as usize * 4 / 10, "{audio} samples");
        assert!(matches!(outputs.last(), Some(Output::End)));
    }

    #[test]
    fn decode_from_a_start_position_drops_earlier_output() {
        let outputs = decode_fixture(250_000);
        assert!(outputs.iter().all(|output| match output {
            Output::Video(frame) => frame.pts_us >= 250_000,
            Output::Audio(block) => block.pts_us >= 250_000,
            Output::End => true,
        }));
        assert!(
            outputs
                .iter()
                .any(|output| matches!(output, Output::Video(_)))
        );
    }

    #[test]
    fn helper_session_streams_the_fixture_through_ipc() {
        use super::super::{ipc, worker::serve_child};
        let descriptor = super::super::ranges::tests::descriptor_for(FIXTURE);
        let (parent_read, mut child_write) = std::io::pipe().unwrap();
        let (mut child_read, parent_write) = std::io::pipe().unwrap();
        let child = std::thread::spawn(move || {
            super::super::helper::serve_on(&mut child_read, &mut child_write, &Contained(()))
        });
        let (sender, receiver) = std::sync::mpsc::sync_channel(64);
        let consumer = std::thread::spawn(move || receiver.into_iter().collect::<Vec<_>>());
        serve_child(
            parent_write,
            parent_read,
            &descriptor,
            super::super::ranges::tests::MemoryChunks {
                bytes: FIXTURE.to_vec(),
                corrupt: None,
                loads: 0,
            },
            ipc::Start {
                descriptor: descriptor.clone(),
                start_us: 0,
            },
            7,
            &sender,
        )
        .unwrap();
        drop(sender);
        child.join().unwrap().unwrap();
        let outputs = consumer.join().unwrap();
        assert!(outputs.iter().any(|output| matches!(
            output,
            Ok(Output::Video(frame)) if frame.generation == 7
        )));
        assert!(matches!(outputs.last(), Some(Ok(Output::End))));
    }

    #[test]
    fn tampered_chunks_fail_the_session_instead_of_ending_cleanly() {
        use super::super::{ipc, worker::serve_child};
        let descriptor = super::super::ranges::tests::descriptor_for(FIXTURE);
        let (parent_read, mut child_write) = std::io::pipe().unwrap();
        let (mut child_read, parent_write) = std::io::pipe().unwrap();
        let child = std::thread::spawn(move || {
            super::super::helper::serve_on(&mut child_read, &mut child_write, &Contained(()))
        });
        let (sender, _receiver) = std::sync::mpsc::sync_channel(64);
        let result = serve_child(
            parent_write,
            parent_read,
            &descriptor,
            super::super::ranges::tests::MemoryChunks {
                bytes: FIXTURE.to_vec(),
                corrupt: Some(0),
                loads: 0,
            },
            ipc::Start {
                descriptor: descriptor.clone(),
                start_us: 0,
            },
            1,
            &sender,
        );
        assert!(result.unwrap_err().to_string().contains("hash mismatch"));
        drop(_receiver);
        let _ = child.join();
    }

    #[test]
    fn loud_opus_with_positive_header_gain_is_bounded_after_decode() {
        let channels = opus::Channels::Mono;
        let mut encoder =
            opus::Encoder::new(SAMPLE_RATE, channels, opus::Application::Audio).unwrap();
        let source: Vec<_> = (0..OPUS_PACKET_FRAMES)
            .map(|i| (i as f32 * 0.1).sin() * 0.95)
            .collect();
        let mut packet = vec![0; 4096];
        let len = encoder.encode_float(&source, &mut packet).unwrap();
        let mut decoder = opus::Decoder::new(SAMPLE_RATE, channels).unwrap();
        decoder.set_gain(12 * 256).unwrap();
        let mut samples = vec![0.0; OPUS_PACKET_FRAMES];
        let count = decoder
            .decode_float(&packet[..len], &mut samples, false)
            .unwrap();
        samples.truncate(count);
        assert!(samples.iter().any(|sample| sample.abs() > 1.0));
        clamp_opus(&mut samples).unwrap();
        PcmBlock {
            generation: 1,
            pts_us: 0,
            channels: 1,
            samples,
        }
        .validate(1)
        .unwrap();
        assert!(clamp_opus(&mut [f32::NAN]).is_err());
        assert!(clamp_opus(&mut [f32::INFINITY]).is_err());
    }
}
