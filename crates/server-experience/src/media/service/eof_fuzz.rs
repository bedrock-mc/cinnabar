//! Seeded fuzz: a non-looping clip always reaches its end exactly once, after its last frame
//! and audio, whatever decoder stalls and rebuffering holds came before end of stream.

use super::{
    super::{
        OPUS_PACKET_FRAMES, SAMPLE_RATE,
        frames::{PcmBlock, VideoFrame},
        timeline::Operation,
    },
    output::Output,
    tests::{control, player},
    *,
};
use std::collections::VecDeque;

const CASES: u64 = 400;
const TICKS: usize = 4000;

/// SplitMix64, so every case replays from its seed.
struct Rng(u64);

impl Rng {
    fn below(&mut self, bound: u64) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        (z ^ (z >> 31)) % bound.max(1)
    }
}

/// Decoder output in presentation order: frames, 20 ms audio packets, then the end.
fn stream(frames: u64, interval: u64, audio_end: u64, generation: u64) -> VecDeque<Output> {
    let packet = OPUS_PACKET_FRAMES as u64 * 1_000_000 / u64::from(SAMPLE_RATE);
    let mut outputs = Vec::new();
    for index in 0..frames {
        outputs.push((
            index * interval,
            Output::Video(VideoFrame {
                generation,
                pts_us: index * interval,
                width: 2,
                height: 2,
                rgba: vec![0; 16],
            }),
        ));
    }
    let mut pts = 0;
    while pts < audio_end {
        outputs.push((
            pts,
            Output::Audio(PcmBlock {
                generation,
                pts_us: pts,
                channels: 1,
                samples: vec![0.0; OPUS_PACKET_FRAMES],
            }),
        ));
        pts += packet;
    }
    outputs.sort_by_key(|(pts, _)| *pts);
    let mut outputs: VecDeque<_> = outputs.into_iter().map(|(_, output)| output).collect();
    outputs.push_back(Output::End);
    outputs
}

fn run_case(seed: u64) {
    let mut rng = Rng(seed);
    let fps = 10 + rng.below(21) as u32;
    let interval = 1_000_000 / u64::from(fps);
    let frames = 3 + rng.below(40);
    let video_end = frames * interval;
    let audio_end = video_end.saturating_sub(interval) + rng.below(400_000);
    let duration = video_end.max(audio_end) + rng.below(100_000);
    let mut player = player();
    player.descriptor.fps = fps;
    player.descriptor.duration_us = duration;
    control(&mut player, 1, 0, Operation::Play { position_us: 0 });
    player.playback.advance(0, duration).unwrap();
    player.decoder_generation = player.playback.decode_generation;
    let mut pending = stream(frames, interval, audio_end, player.decoder_generation);
    let (mut now, mut stalled_until) = (0, 0);
    let mut ended = Vec::new();
    for tick in 0..TICKS {
        now += 1 + rng.below(40_000);
        if !pending.is_empty() && now >= stalled_until && rng.below(6) == 0 {
            stalled_until = now + rng.below(700_000);
        }
        if now >= stalled_until {
            // Sometimes deliver only part of what is ready, so the end can arrive on its own.
            let mut budget = 1 + rng.below(8);
            let generation = player.decoder_generation;
            player.decoder_ended |= player
                .output
                .pump(generation, || {
                    budget = budget.checked_sub(1)?;
                    pending.pop_front().map(Ok)
                })
                .unwrap();
        }
        player.tick(0, now, true).unwrap();
        player.video(now);
        while player.take_pcm().is_some() {}
        for event in player.take_events() {
            if event.kind == EventKind::Ended {
                ended.push((tick, event.position_us));
            }
        }
        if !ended.is_empty() && tick > ended[0].0 + 50 {
            break;
        }
    }
    assert_eq!(
        ended.len(),
        1,
        "seed {seed}: ended {ended:?}, stream left {}",
        pending.len()
    );
    let at = ended[0].1;
    assert!(
        at >= video_end.min(duration) && at >= audio_end.min(duration),
        "seed {seed}: ended at {at} before video {video_end} / audio {audio_end}"
    );
}

#[test]
fn a_clip_always_reaches_its_end_once_after_its_last_frame_and_audio() {
    for seed in 0..CASES {
        run_case(seed);
    }
}
