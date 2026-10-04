//! Vanilla ambient random-number generator.
//! Eager initialization/twisting is equivalent to the native lazy MT19937 state.

use std::{collections::hash_map::RandomState, hash::BuildHasher};

const WORDS: usize = 624;
const TWIST_OFFSET: usize = 397;
const SEED_MULTIPLIER: u32 = 0x6c07_8965;
const TWIST_MATRIX: u32 = 0x9908_b0df;

pub struct AmbientRandom {
    words: [u32; WORDS],
    index: usize,
}

impl Default for AmbientRandom {
    fn default() -> Self {
        // Native obtains its seed from platform entropy. RandomState supplies
        // locally randomized OS-seeded keys without adding a runtime dependency.
        Self::new(RandomState::new().hash_one(()) as u32)
    }
}

impl AmbientRandom {
    pub(crate) fn new(seed: u32) -> Self {
        let mut words = [0; WORDS];
        words[0] = seed;
        for index in 1..WORDS {
            let previous = words[index - 1];
            words[index] = (previous ^ (previous >> 30))
                .wrapping_mul(SEED_MULTIPLIER)
                .wrapping_add(index as u32);
        }
        Self {
            words,
            index: WORDS,
        }
    }

    pub(crate) fn next(&mut self) -> u32 {
        if self.index == WORDS {
            for index in 0..WORDS {
                let joined = (self.words[index] & 0x8000_0000)
                    | (self.words[(index + 1) % WORDS] & 0x7fff_ffff);
                self.words[index] = self.words[(index + TWIST_OFFSET) % WORDS]
                    ^ (joined >> 1)
                    ^ if joined & 1 == 0 { 0 } else { TWIST_MATRIX };
            }
            self.index = 0;
        }
        let mut value = self.words[self.index];
        self.index += 1;
        value ^= value >> 11;
        value ^= (value & 0x013a_58ad) << 7;
        value ^= (value & 0x0001_df8c) << 15;
        value ^ (value >> 18)
    }

    pub fn bounded(&mut self, upper: u32) -> u32 {
        if upper == 0 { 0 } else { self.next() % upper }
    }

    /// Native calls this distribution Z, Y, X, consuming two words per axis.
    pub(crate) fn gaussian_int(&mut self, radius: u32) -> i32 {
        self.bounded(radius) as i32 - self.bounded(radius) as i32
    }
}
