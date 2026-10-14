//! Shared policy of the opt-in distant terrain extension: the Video setting's choices and
//! the per-device caps the settings, the session and the renderer read.

/// Mebibyte, the unit of the storage and GPU caps.
pub const MIB: u64 = 1 << 20;

/// Detail kept on screen; each step halves the pixels a section edge may span before it
/// refines.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
#[repr(i32)]
pub enum DistantTerrainQuality {
    /// Provisional: not measured on hardware yet.
    Low = 0,
    #[default]
    Medium = 1,
    High = 2,
}

impl DistantTerrainQuality {
    /// Every quality in persisted order.
    pub const ALL: [Self; 3] = [Self::Low, Self::Medium, Self::High];

    /// The persisted value.
    #[must_use]
    pub const fn index(self) -> i32 {
        self as i32
    }

    /// Decodes a persisted value; unknown values read as the default.
    #[must_use]
    pub const fn from_index(value: i32) -> Self {
        match value {
            0 => Self::Low,
            2 => Self::High,
            _ => Self::Medium,
        }
    }

    /// The caption shared by the Video settings hosts.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
        }
    }
}

/// Nearest distant terrain drawing distance the slider offers, in chunks.
pub const MIN_DISTANT_TERRAIN_CHUNKS: u32 = 32;
/// Drawing distance a first enable uses, in chunks.
pub const DEFAULT_DISTANT_TERRAIN_CHUNKS: u32 = 256;
/// Steps of the per-server storage slider, in MiB.
pub const DISTANT_TERRAIN_STORAGE_STEP_MIB: u32 = 256;
/// Smallest per-server storage cap the slider offers, in MiB.
pub const MIN_DISTANT_TERRAIN_STORAGE_MIB: u32 = DISTANT_TERRAIN_STORAGE_STEP_MIB;
/// Per-server storage cap a first enable uses, in MiB.
pub const DEFAULT_DISTANT_TERRAIN_STORAGE_MIB: u32 = 1024;
/// Largest per-server storage cap the slider offers, in MiB: the cache's total cap.
pub const MAX_DISTANT_TERRAIN_STORAGE_MIB: u32 = 8192;

const _: () =
    assert!(DEFAULT_DISTANT_TERRAIN_STORAGE_MIB.is_multiple_of(DISTANT_TERRAIN_STORAGE_STEP_MIB));
const _: () =
    assert!(MAX_DISTANT_TERRAIN_STORAGE_MIB.is_multiple_of(DISTANT_TERRAIN_STORAGE_STEP_MIB));

/// Level rings within [`MIN_DISTANT_TERRAIN_CHUNKS`]: the distance the finest levels cover
/// before the first doubling.
const INNER_RINGS: u32 = 2;

/// Level rings a drawing distance of `chunks` reaches: each level covers twice the distance
/// of the one below with about as many sections, so the drawn geometry grows by about one
/// ring per doubling of the distance. Distances round up to the next doubling.
const fn distance_rings(chunks: u32) -> u32 {
    let doublings = chunks
        .div_ceil(MIN_DISTANT_TERRAIN_CHUNKS)
        .next_power_of_two()
        .trailing_zeros();
    doublings + INNER_RINGS
}

/// Caps of one class of device. Pools are allocated once when distant terrain is enabled,
/// never grown, and further capped by the adapter's buffer limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DistantTerrainTier {
    /// Farthest drawing distance in chunks.
    pub max_distance_chunks: u32,
    /// Geometry pool bytes at [`Self::max_distance_chunks`] by
    /// [`DistantTerrainQuality::index`].
    pub pool_bytes: [u64; 3],
}

impl DistantTerrainTier {
    /// Desktop GPUs: Medium 256 MiB, High 512 MiB; Low is provisional.
    pub const DESKTOP: Self = Self {
        max_distance_chunks: 512,
        pool_bytes: [128 * MIB, 256 * MIB, 512 * MIB],
    };
    /// Phones, as the experimental Android client runs on, and low-end desktop adapters.
    pub const LOW_END: Self = Self {
        max_distance_chunks: 256,
        pool_bytes: [64 * MIB, 128 * MIB, 128 * MIB],
    };
    /// The tier of the platform this build targets; a renderer may choose
    /// [`Self::LOW_END`] for a low-end adapter of a desktop build.
    pub const CURRENT: Self = if cfg!(target_os = "android") {
        Self::LOW_END
    } else {
        Self::DESKTOP
    };

    /// The geometry pool bytes of `quality` at a drawing distance of `distance_chunks`:
    /// the tier's pool at its farthest distance, scaled by the level rings the distance
    /// reaches ([`distance_rings`]) and rounded up to whole MiB. Provisional: the scale
    /// follows the spike's resident counts at Medium (about 104 MB at 256 chunks, 122 MB at
    /// 1024) and is unmeasured below 256 chunks.
    #[must_use]
    pub const fn pool_bytes(self, quality: DistantTerrainQuality, distance_chunks: u32) -> u64 {
        let full = self.pool_bytes[quality as usize] / MIB;
        let rings = distance_rings(self.clamp_distance(distance_chunks)) as u64;
        let all = distance_rings(self.max_distance_chunks) as u64;
        (full * rings).div_ceil(all) * MIB
    }

    /// `chunks` within the slider's range and this tier's distance.
    #[must_use]
    pub const fn clamp_distance(self, chunks: u32) -> u32 {
        let low = if chunks < MIN_DISTANT_TERRAIN_CHUNKS {
            MIN_DISTANT_TERRAIN_CHUNKS
        } else {
            chunks
        };
        if low > self.max_distance_chunks {
            self.max_distance_chunks
        } else {
            low
        }
    }
}

const _: () =
    assert!(DEFAULT_DISTANT_TERRAIN_CHUNKS <= DistantTerrainTier::LOW_END.max_distance_chunks);
const _: () = assert!(
    DistantTerrainTier::LOW_END.max_distance_chunks
        <= DistantTerrainTier::DESKTOP.max_distance_chunks
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qualities_round_trip_and_unknown_values_read_medium() {
        for quality in DistantTerrainQuality::ALL {
            assert_eq!(DistantTerrainQuality::from_index(quality.index()), quality);
            assert!(!quality.label().is_empty());
        }
        for invalid in [-1, 3, i32::MAX] {
            assert_eq!(
                DistantTerrainQuality::from_index(invalid),
                DistantTerrainQuality::Medium
            );
        }
    }

    #[test]
    fn tiers_keep_finer_qualities_no_smaller_and_clamp_distances() {
        for tier in [DistantTerrainTier::DESKTOP, DistantTerrainTier::LOW_END] {
            assert!(tier.pool_bytes.is_sorted());
            assert_eq!(tier.clamp_distance(0), MIN_DISTANT_TERRAIN_CHUNKS);
            assert_eq!(tier.clamp_distance(u32::MAX), tier.max_distance_chunks);
            assert_eq!(
                tier.clamp_distance(DEFAULT_DISTANT_TERRAIN_CHUNKS),
                DEFAULT_DISTANT_TERRAIN_CHUNKS
            );
        }
        let high = DistantTerrainQuality::High;
        assert!(
            DistantTerrainTier::LOW_END.pool_bytes(high, u32::MAX)
                < DistantTerrainTier::DESKTOP.pool_bytes(high, u32::MAX)
        );
    }

    #[test]
    fn pools_grow_with_distance_up_to_the_tier_pool() {
        for tier in [DistantTerrainTier::DESKTOP, DistantTerrainTier::LOW_END] {
            for quality in DistantTerrainQuality::ALL {
                let full = tier.pool_bytes[quality.index() as usize];
                assert_eq!(tier.pool_bytes(quality, tier.max_distance_chunks), full);
                assert_eq!(tier.pool_bytes(quality, u32::MAX), full);
                let mut last = 0;
                for chunks in MIN_DISTANT_TERRAIN_CHUNKS..=tier.max_distance_chunks {
                    let bytes = tier.pool_bytes(quality, chunks);
                    assert!(bytes >= last && bytes <= full, "{quality:?} at {chunks}");
                    assert!(bytes.is_multiple_of(MIB));
                    last = bytes;
                }
                assert!(
                    tier.pool_bytes(quality, MIN_DISTANT_TERRAIN_CHUNKS) < full / 2,
                    "the nearest distance needs well under the farthest's pool"
                );
            }
        }
        // Only a new doubling of the distance changes the pool, so a slider drag rebuilds
        // the pool a few times at most.
        let medium = DistantTerrainQuality::Medium;
        let desktop = DistantTerrainTier::DESKTOP;
        assert_eq!(
            desktop.pool_bytes(medium, 129),
            desktop.pool_bytes(medium, 256)
        );
        assert!(desktop.pool_bytes(medium, 128) < desktop.pool_bytes(medium, 129));
    }
}
