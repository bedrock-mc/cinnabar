//! Independent, bounded presentation queues for interleaved decoder output.

use super::super::frames::{FrameQueue, PcmBlock};
use super::super::{MAX_FRAMES, frames::VideoFrame};
use anyhow::{Result, ensure};
use std::collections::VecDeque;

const MAX_AUDIO_BLOCKS: usize = super::super::MAX_PCM_FRAMES / super::super::OPUS_PACKET_FRAMES;
const MAX_PUMP_OUTPUTS: usize = MAX_FRAMES + MAX_AUDIO_BLOCKS;

#[derive(Debug)]
pub enum Output {
    Video(VideoFrame),
    Audio(PcmBlock),
    End,
}

#[derive(Default)]
pub(super) struct Queues {
    pub(super) frames: FrameQueue,
    pub(super) pcm: VecDeque<PcmBlock>,
    /// Presentation end of the latest decoded audio.
    pub(super) audio_end_us: u64,
}

impl Queues {
    /// Drains interleaved output in bounded batches, retaining each stream until its consumer
    /// takes it; returns whether the decoder reported its end.
    pub(super) fn pump(
        &mut self,
        generation: u64,
        mut poll: impl FnMut() -> Option<Result<Output>>,
    ) -> Result<bool> {
        let mut ended = false;
        for _ in 0..MAX_PUMP_OUTPUTS {
            if !self.frames.has_capacity() || self.pcm.len() >= MAX_AUDIO_BLOCKS {
                break;
            }
            let Some(output) = poll() else {
                break;
            };
            match output? {
                Output::Video(frame) => self.frames.push(frame, generation)?,
                Output::Audio(block) => {
                    block.validate(generation)?;
                    ensure!(
                        block.samples.len() / usize::from(block.channels)
                            <= super::super::OPUS_PACKET_FRAMES,
                        "PCM block exceeds packet frame limit"
                    );
                    let frames = (block.samples.len() / usize::from(block.channels)) as u64;
                    self.audio_end_us = self.audio_end_us.max(
                        block.pts_us + frames * 1_000_000 / u64::from(super::super::SAMPLE_RATE),
                    );
                    self.pcm.push_back(block);
                }
                Output::End => ended = true,
            }
        }
        Ok(ended)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maximum_packet_blocks_cannot_exceed_the_pcm_duration_ceiling() {
        for channels in [1, 2] {
            let mut queues = Queues::default();
            queues
                .pump(1, || {
                    Some(Ok(Output::Audio(PcmBlock {
                        generation: 1,
                        pts_us: 0,
                        channels,
                        samples: vec![
                            0.0;
                            super::super::super::OPUS_PACKET_FRAMES
                                * usize::from(channels)
                        ],
                    })))
                })
                .unwrap();
            assert_eq!(queues.pcm.len(), MAX_AUDIO_BLOCKS);
            let frames: usize = queues
                .pcm
                .iter()
                .map(|block| block.samples.len() / usize::from(block.channels))
                .sum();
            assert!(frames <= super::super::super::MAX_PCM_FRAMES);
            queues.pcm.clear();
            let block = PcmBlock {
                generation: 1,
                pts_us: 0,
                channels,
                samples: vec![0.0; super::super::super::MAX_PCM_FRAMES * usize::from(channels)],
            };
            block.validate(1).unwrap();
            let mut output = Some(Ok(Output::Audio(block)));
            assert!(queues.pump(1, || output.take()).is_err());
            assert!(queues.pcm.is_empty());
        }
    }

    #[test]
    fn sixty_hz_pump_sustains_thirty_fps_and_twenty_ms_audio() {
        let packet_us = super::super::super::OPUS_PACKET_FRAMES as u64 * 1_000_000
            / u64::from(super::super::super::SAMPLE_RATE);
        let mut queues = Queues::default();
        let mut pending = VecDeque::new();
        let (mut audio, mut video, mut played_audio, mut played_video) = (0, 0, 0, 0);
        for tick in 0..=600 {
            let now = tick * 1_000_000 / 60;
            while audio * packet_us <= now {
                pending.push_back(Output::Audio(PcmBlock {
                    generation: 1,
                    pts_us: audio * packet_us,
                    channels: 1,
                    samples: vec![0.0; super::super::super::OPUS_PACKET_FRAMES],
                }));
                audio += 1;
            }
            while video * 1_000_000 / u64::from(super::super::super::MAX_FPS) <= now {
                pending.push_back(Output::Video(VideoFrame {
                    generation: 1,
                    pts_us: video * 1_000_000 / u64::from(super::super::super::MAX_FPS),
                    width: 1,
                    height: 1,
                    rgba: vec![0; 4],
                }));
                video += 1;
            }
            queues.pump(1, || pending.pop_front().map(Ok)).unwrap();
            if queues.pcm.pop_front().is_some() {
                played_audio += 1;
            }
            if queues.frames.present(now, 1).is_some() {
                played_video += 1;
            }
            assert!(pending.is_empty(), "decoder backlog grew at tick {tick}");
        }
        assert_eq!(played_audio, audio);
        assert_eq!(played_video, video);
    }
    #[test]
    fn a_tick_never_drains_an_unbounded_output_stream() {
        let mut queues = Queues::default();
        let mut polls = 0;
        queues
            .pump(1, || {
                polls += 1;
                Some(Ok(Output::End))
            })
            .unwrap();
        assert_eq!(polls, MAX_PUMP_OUTPUTS);
    }

    #[test]
    fn queues_stop_polling_at_each_independent_ceiling() {
        let mut queues = Queues::default();
        let mut polls = 0;
        queues
            .pump(1, || {
                polls += 1;
                Some(Ok(Output::Audio(PcmBlock {
                    generation: 1,
                    pts_us: 0,
                    channels: 1,
                    samples: vec![0.0],
                })))
            })
            .unwrap();
        assert_eq!(polls, MAX_AUDIO_BLOCKS);
        assert_eq!(queues.pcm.len(), MAX_AUDIO_BLOCKS);
        assert!(queues.frames.has_capacity());
        queues.pcm.clear();
        polls = 0;
        queues
            .pump(1, || {
                polls += 1;
                Some(Ok(Output::Video(VideoFrame {
                    generation: 1,
                    pts_us: 0,
                    width: 1,
                    height: 1,
                    rgba: vec![0; 4],
                })))
            })
            .unwrap();
        assert_eq!(polls, MAX_FRAMES);
        assert!(queues.pcm.is_empty());
    }
}
