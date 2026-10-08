//! Manifest memory requirements and device-compatible subpack selection.

use crate::Subpack;
use serde_json::Value;

/// Converts physical memory bytes to vanilla's tier with strict upper boundaries.
pub(crate) fn device_memory_tier(bytes: u64) -> u32 {
    [2, 4, 6, 8, 12]
        .into_iter()
        .filter(|gib| bytes > gib * (1 << 30))
        .count() as u32
}

/// Chooses the last highest supported tier, falling back to the pack root.
pub(crate) fn select(subpacks: &[Subpack], memory: u32) -> &str {
    subpacks
        .iter()
        .filter(|pack| pack.memory_tier <= memory)
        .max_by_key(|pack| pack.memory_tier)
        .map_or("", |pack| pack.folder.as_str())
}

/// Legacy requirements use a different scale from the modern performance tier.
pub(crate) fn manifest_memory_tier(subpack: &Value) -> u32 {
    let integer = |key: &str| {
        subpack[key]
            .as_i64()
            .and_then(|value| i32::try_from(value).ok())
    };
    if let Some(legacy) = integer("memory_tier") {
        [11, 12, 18, 24, 32]
            .into_iter()
            .filter(|minimum| legacy >= *minimum)
            .count() as u32
    } else {
        integer("memory_performance_tier")
            .unwrap_or_default()
            .clamp(0, 5) as u32
    }
}

/// Keeps a supported explicit selection; otherwise chooses the best supported option.
pub(crate) fn supported<'a>(subpacks: &'a [Subpack], requested: &str, memory: u32) -> &'a str {
    subpacks
        .iter()
        .find(|pack| pack.folder == requested && pack.memory_tier <= memory)
        .map_or_else(|| select(subpacks, memory), |pack| pack.folder.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_hardware_boundaries_and_largest_tier() {
        for (tier, gib) in [2, 4, 6, 8, 12].into_iter().enumerate() {
            let bytes = gib * (1 << 30);
            assert_eq!(device_memory_tier(bytes - 1), tier as u32);
            assert_eq!(device_memory_tier(bytes), tier as u32);
            assert_eq!(device_memory_tier(bytes + 1), tier as u32 + 1);
        }
        assert_eq!(device_memory_tier(u64::MAX), 5);
    }

    #[test]
    fn legacy_and_modern_manifest_memory_scales_remain_distinct() {
        for (value, expected) in [
            (-1, 0),
            (0, 0),
            (10, 0),
            (11, 1),
            (12, 2),
            (17, 2),
            (18, 3),
            (23, 3),
            (24, 4),
            (31, 4),
            (32, 5),
            (100, 5),
        ] {
            assert_eq!(
                manifest_memory_tier(&serde_json::json!({"memory_tier": value})),
                expected
            );
        }
        for (value, expected) in [(-1, 0), (0, 0), (2, 2), (5, 5), (12, 5)] {
            assert_eq!(
                manifest_memory_tier(&serde_json::json!({"memory_performance_tier": value})),
                expected
            );
        }
        assert_eq!(
            manifest_memory_tier(
                &serde_json::json!({"memory_tier": 12, "memory_performance_tier": 4})
            ),
            2
        );
        assert_eq!(
            manifest_memory_tier(
                &serde_json::json!({"memory_tier": "bad", "memory_performance_tier": 4})
            ),
            4
        );
    }

    #[test]
    fn automatic_selection_uses_last_supported_highest_tier() {
        let packs = [("high", 4), ("low", 0), ("medium", 2), ("last", 2)]
            .into_iter()
            .map(|(folder, memory_tier)| Subpack {
                folder: folder.into(),
                name: folder.into(),
                memory_tier,
            })
            .collect::<Vec<_>>();
        assert_eq!(select(&packs, 1), "low");
        assert_eq!(select(&packs, 2), "last");
        assert_eq!(select(&packs, 5), "high");
        assert_eq!(select(&packs[..1], 0), "");
    }
}
