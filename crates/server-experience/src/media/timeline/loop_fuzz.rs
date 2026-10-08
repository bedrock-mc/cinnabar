//! Seeded fuzz: decoding restarts exactly once per real loop crossing, and the shown position
//! follows the authored timeline, whatever the ticks, rebuffering holds, scheduled or late
//! controls and seeks around it, for looping and non-looping clips.

use super::*;
use crate::policy::INITIAL_BUNDLE_GENERATION;

const CASES: u64 = 400;
const TICKS: usize = 300;

/// SplitMix64, so every case replays from its seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound.max(1)
    }
}

/// The specification: a playhead in unwrapped media time that advances with time while
/// playing and not held; controls set or keep it; the shown position wraps into the loop.
struct Reference {
    looping: bool,
    playhead: u64,
    playing: bool,
    held: bool,
    at: u64,
    start: u64,
    len: u64,
    duration: u64,
    crossings: u64,
}

impl Reference {
    fn iteration(&self, playhead: u64) -> u64 {
        if self.looping {
            playhead.saturating_sub(self.start) / self.len
        } else {
            0
        }
    }

    /// A late control's authored time may precede time already played; that adds nothing.
    fn run_to(&mut self, at: u64) {
        if at < self.at {
            return;
        }
        if self.playing && !self.held {
            let next = self.playhead + (at - self.at);
            self.crossings += self.iteration(next) - self.iteration(self.playhead);
            self.playhead = next;
        }
        self.at = at;
    }

    /// Applies one control authored at `at`; reports whether it restarts decoding by itself.
    /// A position control lands where it would have had it arrived on time.
    fn apply(&mut self, at: u64, operation: &Operation) -> bool {
        self.held = false;
        if matches!(
            operation,
            Operation::Play { .. } | Operation::Pause { .. } | Operation::Seek { .. }
        ) {
            self.at = at;
        }
        match *operation {
            Operation::Play { position_us } => {
                self.playhead = position_us.min(self.duration);
                self.playing = true;
                true
            }
            Operation::Pause { position_us } => {
                self.playhead = position_us.min(self.duration);
                self.playing = false;
                false
            }
            Operation::Seek { position_us } => {
                self.playhead = position_us.min(self.duration);
                true
            }
            _ => false,
        }
    }

    fn shown(&self) -> u64 {
        if self.looping && self.playhead >= self.start + self.len {
            self.start + (self.playhead - self.start) % self.len
        } else {
            self.playhead.min(self.duration)
        }
    }
}

fn control(rng: &mut Rng, duration: u64, loop_end: u64) -> Operation {
    let position_us = match rng.below(3) {
        0 => rng.below(duration + 1),
        1 => loop_end.saturating_sub(rng.below(50_000)),
        _ => rng.below(loop_end),
    };
    match rng.below(4) {
        0 => Operation::Play { position_us },
        1 => Operation::Pause { position_us },
        2 => Operation::Seek { position_us },
        _ => Operation::SetVolume {
            per_mille: rng.below(1001) as u16,
        },
    }
}

fn run_case(seed: u64) {
    let mut rng = Rng(seed);
    let len = if rng.below(2) == 0 {
        20_000 + rng.below(480_000)
    } else {
        1_000_000 + rng.below(9_000_000)
    };
    let looping = rng.below(4) != 0;
    let start = rng.below(1_000_000);
    let duration = start + len + rng.below(2_000_000);
    let owner = Principal {
        session: "session".into(),
        bundle: "cinema".into(),
        generation: INITIAL_BUNDLE_GENERATION,
    };
    let mut playback = Playback::default();
    let mut revision = 0;
    let mut send = |playback: &mut Playback, at: u64, now: u64, operation: Operation| {
        revision += 1;
        let message = Message {
            owner: owner.clone(),
            instance: INITIAL_MEDIA_INSTANCE,
            generation: INITIAL_MEDIA_GENERATION,
            timeline: "cinema".into(),
            world_epoch: 1,
            revision,
            effective_server_us: at,
            operation,
        };
        playback.enqueue(message, &owner, 1, "cinema", now).unwrap();
    };
    if looping {
        send(
            &mut playback,
            0,
            0,
            Operation::SetLoop {
                bounds_us: Some([start, start + len]),
            },
        );
    }
    let first = rng.below(duration);
    send(&mut playback, 0, 0, Operation::Play { position_us: first });
    playback.advance(0, duration).unwrap();
    let mut reference = Reference {
        looping,
        playhead: first.min(duration),
        playing: true,
        held: false,
        at: 0,
        start,
        len,
        duration,
        crossings: 0,
    };
    let mut scheduled: VecDeque<(u64, Operation)> = VecDeque::new();
    let (mut now, mut last_effective) = (0, 0);
    for tick in 0..TICKS {
        let step = 1 + rng.below(len / 2);
        let previous = now;
        now += step;
        for _ in 0..rng.below(3) {
            if rng.below(3) != 0 {
                continue;
            }
            // Late (authored before time already played), due now, or scheduled ahead.
            let at = if last_effective < previous && rng.below(3) == 0 {
                last_effective + rng.below(previous - last_effective)
            } else {
                (previous + rng.below(2 * step)).max(last_effective)
            };
            last_effective = at;
            let operation = control(&mut rng, duration, start + len);
            send(&mut playback, at, now, operation.clone());
            scheduled.push_back((at, operation));
        }
        let before = playback.decode_generation;
        playback.advance(now, duration).unwrap();
        let restarts = playback.decode_generation - before;

        reference.crossings = 0;
        let mut resets = 0;
        while scheduled.front().is_some_and(|(at, _)| *at <= now) {
            let (at, operation) = scheduled.pop_front().unwrap();
            reference.run_to(at);
            resets += u64::from(reference.apply(at, &operation));
        }
        reference.run_to(now);
        let expected = resets + u64::from(resets == 0 && reference.crossings > 0);
        assert_eq!(
            restarts, expected,
            "seed {seed} tick {tick}: {} loop crossings, {resets} resetting controls",
            reference.crossings
        );
        assert_eq!(
            playback.position(now, duration),
            reference.shown(),
            "seed {seed} tick {tick}: shown position"
        );
        match rng.below(10) {
            0..=2 => {
                playback.hold(now, duration);
                reference.held = true;
            }
            3 => {
                playback.release();
                reference.held = false;
            }
            _ => {}
        }
    }
}

#[test]
fn every_real_loop_crossing_restarts_decoding_once_and_nothing_else_does() {
    for seed in 0..CASES {
        run_case(seed);
    }
}
