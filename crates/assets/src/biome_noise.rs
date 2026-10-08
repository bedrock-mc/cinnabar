//! Bedrock grass-noise permutation.

const MT_WORDS: usize = 624;
const PERMUTATION_SIZE: usize = 256;

/// Reproduces the client's seeded MT stream without platform RNG dependencies.
pub struct ClientRandom {
    state: [u32; MT_WORDS],
    index: usize,
}

impl ClientRandom {
    /// Initializes the complete state equivalent to the client's lazy seed expansion.
    pub fn new(seed: u32) -> Self {
        let mut state = [0; MT_WORDS];
        state[0] = seed;
        for i in 1..state.len() {
            state[i] = (state[i - 1] ^ (state[i - 1] >> 30))
                .wrapping_mul(0x6c07_8965)
                .wrapping_add(i as u32);
        }
        Self {
            state,
            index: MT_WORDS,
        }
    }

    /// Twists one word and applies the current client's tempering operations.
    pub fn next_u32(&mut self) -> u32 {
        let i = self.index % self.state.len();
        let bits = (self.state[i] & 0x8000_0000) | (self.state[(i + 1) % MT_WORDS] & 0x7fff_ffff);
        let mut value = self.state[(i + 397) % MT_WORDS]
            ^ (bits >> 1)
            ^ if bits & 1 != 0 { 0x9908_b0df } else { 0 };
        self.state[i] = value;
        self.index = i + 1;
        value ^= value >> 11;
        value ^= (value & 0x013a_58ad) << 7;
        value ^= (value & 0x0001_df8c) << 15;
        value ^ (value >> 18)
    }

    /// Converts the full unsigned word to a float as vanilla's random float does.
    pub fn next_float(&mut self) -> f32 {
        (f64::from(self.next_u32()) / 4_294_967_296.0) as f32
    }
}

/// Builds the one-octave seed-2345 permutation, including its three offset draws.
pub fn grass_noise_permutation() -> [u32; PERMUTATION_SIZE] {
    let mut random = ClientRandom::new(2345);
    for _ in 0..3 {
        random.next_float();
    }
    let mut permutation = std::array::from_fn(|i| i as u32);
    for i in 0..permutation.len() {
        let selected = i + random.next_u32() as usize % (PERMUTATION_SIZE - i);
        permutation.swap(i, selected);
    }
    permutation
}
