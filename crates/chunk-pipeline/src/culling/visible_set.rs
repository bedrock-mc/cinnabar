use hashbrown::HashSet;

use world::SubChunkKey;

use super::grid::cell_index;

/// Cave-visible sub-chunks: a bitset over a camera-centred window with the connectivity
/// grid's dimensions, plus a small set for keys outside that window.
#[derive(Debug, Clone, Default)]
pub struct CaveVisibleSet {
    dimension: i32,
    origin: [i64; 3],
    xz_bits: u32,
    y_bits: u32,
    bits: Vec<u64>,
    members: Vec<SubChunkKey>,
    outside: HashSet<SubChunkKey>,
}

impl CaveVisibleSet {
    #[must_use]
    pub fn contains(&self, key: &SubChunkKey) -> bool {
        match self.window_index(*key) {
            Some(index) => self.bits[index / 64] & (1 << (index % 64)) != 0,
            None => self.outside.contains(key),
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.members.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = SubChunkKey> + '_ {
        self.members.iter().copied()
    }

    /// Empties the set and recentres its window on `camera` with the grid's axis sizes.
    pub(super) fn reset(&mut self, camera: SubChunkKey, (xz_bits, y_bits): (u32, u32)) {
        let words = (1_usize << (2 * xz_bits + y_bits)).div_ceil(64);
        if (self.xz_bits, self.y_bits) == (xz_bits, y_bits) && self.bits.len() == words {
            for key in &self.members {
                if let Some(index) = self.window_index(*key) {
                    self.bits[index / 64] = 0;
                }
            }
        } else {
            self.bits.clear();
            self.bits.resize(words, 0);
        }
        self.members.clear();
        self.outside.clear();
        let half_xz = i64::from(1_u32 << xz_bits) / 2;
        let half_y = i64::from(1_u32 << y_bits) / 2;
        self.dimension = camera.dimension;
        self.origin = [
            i64::from(camera.x) - half_xz,
            i64::from(camera.y) - half_y,
            i64::from(camera.z) - half_xz,
        ];
        self.xz_bits = xz_bits;
        self.y_bits = y_bits;
    }

    pub(super) fn insert(&mut self, key: SubChunkKey) -> bool {
        let inserted = match self.window_index(key) {
            Some(index) => {
                let word = &mut self.bits[index / 64];
                let bit = 1 << (index % 64);
                let fresh = *word & bit == 0;
                *word |= bit;
                fresh
            }
            None => self.outside.insert(key),
        };
        if inserted {
            self.members.push(key);
        }
        inserted
    }

    /// Window positions map one-to-one onto toroidal cells, so no key check is needed.
    fn window_index(&self, key: SubChunkKey) -> Option<usize> {
        if self.bits.is_empty() || key.dimension != self.dimension {
            return None;
        }
        let xz = 1_i64 << self.xz_bits;
        let y = 1_i64 << self.y_bits;
        let inside =
            |value: i32, origin: i64, size: i64| (0..size).contains(&(i64::from(value) - origin));
        (inside(key.x, self.origin[0], xz)
            && inside(key.y, self.origin[1], y)
            && inside(key.z, self.origin[2], xz))
        .then(|| cell_index(self.xz_bits, self.y_bits, key))
    }
}

impl PartialEq for CaveVisibleSet {
    fn eq(&self, other: &Self) -> bool {
        if self.len() != other.len() {
            return false;
        }
        let same_window = (self.dimension, self.origin, self.xz_bits, self.y_bits)
            == (other.dimension, other.origin, other.xz_bits, other.y_bits);
        if same_window {
            self.bits == other.bits && self.outside == other.outside
        } else {
            self.members.iter().all(|key| other.contains(key))
        }
    }
}

impl Eq for CaveVisibleSet {}

impl FromIterator<SubChunkKey> for CaveVisibleSet {
    fn from_iter<I: IntoIterator<Item = SubChunkKey>>(keys: I) -> Self {
        let mut set = Self::default();
        for key in keys {
            set.insert(key);
        }
        set
    }
}
