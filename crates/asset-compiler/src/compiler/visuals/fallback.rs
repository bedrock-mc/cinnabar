use std::sync::OnceLock;

use super::super::*;
use super::context::{
    CuboidTemplateKey, ModelStorage, RuleInputs, diagnostic_visual, intern_cuboid_template,
    set_model_visual,
};
use super::dispatcher::CompileRuleResult;
const ALPHA_CUTOUT: u8 = 1;
const ALPHA_BLEND: u8 = 2;
const MAGIC: &[u8; 8] = b"CVFB1001";
/// The one table format version this compiler consumes.
const FORMAT_VERSION: u32 = 1;
const HEADER_BYTES: usize = 16;
const ENTRY_BYTES: usize = 25;
/// The embedded table can never describe more identities than the bounded
/// registry itself may carry.
const MAX_ENTRY_COUNT: usize = 65_536;
const FALLBACK_BYTES: &[u8] = include_bytes!("../../../data/vanilla-fallback-v1001.bin");
const _: () = assert!(FALLBACK_BYTES.len() >= HEADER_BYTES);

fn identity_fingerprint(record: &RegistryRecord) -> u64 {
    let mut hash = 14_695_981_039_346_656_037_u64;
    for byte in record
        .name
        .bytes()
        .chain(std::iter::once(0))
        .chain(record.canonical_state.bytes())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(1_099_511_628_211);
    }
    hash
}

/// One parsed vanilla-fallback inventory bound to its validated byte span.
///
/// The entry count comes from the table header instead of a compile-time
/// constant, so a sibling table for another registry protocol (with a
/// different count) parses through the same code path while magic, format
/// version, and exact length stay strictly validated.
#[derive(Debug)]
struct FallbackInventory<'a> {
    bytes: &'a [u8],
    entry_count: usize,
}

impl<'a> FallbackInventory<'a> {
    fn parse(bytes: &'a [u8]) -> Result<Self, AssetError> {
        if bytes.len() < HEADER_BYTES || &bytes[..8] != MAGIC {
            return Err(invalid_fallback("invalid vanilla-fallback inventory magic"));
        }
        if u32_at(bytes, 8) != FORMAT_VERSION {
            return Err(invalid_fallback(
                "unsupported vanilla-fallback inventory version",
            ));
        }
        let entry_count = u32_at(bytes, 12) as usize;
        if entry_count > MAX_ENTRY_COUNT {
            return Err(invalid_fallback(
                "vanilla-fallback inventory entry count exceeds the limit",
            ));
        }
        let expected_len = HEADER_BYTES
            + entry_count
                .checked_mul(ENTRY_BYTES)
                .ok_or_else(|| invalid_fallback("vanilla-fallback inventory length overflows"))?;
        if bytes.len() != expected_len {
            return Err(invalid_fallback(
                "vanilla-fallback inventory length does not match its header entry count",
            ));
        }
        Ok(Self { bytes, entry_count })
    }

    fn u16_at(&self, offset: usize) -> u16 {
        u16::from_le_bytes([self.bytes[offset], self.bytes[offset + 1]])
    }

    fn u32_at(&self, offset: usize) -> u32 {
        u32::from_le_bytes([
            self.bytes[offset],
            self.bytes[offset + 1],
            self.bytes[offset + 2],
            self.bytes[offset + 3],
        ])
    }

    fn u64_at(&self, offset: usize) -> u64 {
        u64::from_le_bytes([
            self.bytes[offset],
            self.bytes[offset + 1],
            self.bytes[offset + 2],
            self.bytes[offset + 3],
            self.bytes[offset + 4],
            self.bytes[offset + 5],
            self.bytes[offset + 6],
            self.bytes[offset + 7],
        ])
    }

    fn entry_at(&self, index: usize) -> (u32, u64, [i16; 3], [i16; 3], u8) {
        let offset = HEADER_BYTES + index * ENTRY_BYTES;
        let coordinate = |relative| self.u16_at(offset + relative) as i16;
        (
            self.u32_at(offset),
            self.u64_at(offset + 4),
            [coordinate(12), coordinate(14), coordinate(16)],
            [coordinate(18), coordinate(20), coordinate(22)],
            self.bytes[offset + 24],
        )
    }

    fn entry(&self, record: &RegistryRecord) -> Option<([i16; 3], [i16; 3], u8)> {
        let mut left = 0;
        let mut right = self.entry_count;
        while left < right {
            let middle = left + (right - left) / 2;
            match self.entry_at(middle).0.cmp(&record.network_hash) {
                std::cmp::Ordering::Less => left = middle + 1,
                std::cmp::Ordering::Greater => right = middle,
                std::cmp::Ordering::Equal => {
                    let (_, fingerprint, min, max, alpha) = self.entry_at(middle);
                    return (fingerprint == identity_fingerprint(record))
                        .then_some((min, max, alpha));
                }
            }
        }
        None
    }
}

fn invalid_fallback(detail: &'static str) -> AssetError {
    AssetError::InvalidCompiledAssets {
        detail: detail.into(),
    }
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

/// The embedded pinned protocol-1001 inventory, parsed once.
fn inventory() -> &'static FallbackInventory<'static> {
    static INVENTORY: OnceLock<FallbackInventory<'static>> = OnceLock::new();
    INVENTORY.get_or_init(|| {
        FallbackInventory::parse(FALLBACK_BYTES)
            .expect("embedded vanilla-fallback inventory must parse")
    })
}

pub(in crate::compiler) fn is_record(record: &RegistryRecord) -> bool {
    inventory().entry(record).is_some()
}

pub(in crate::compiler) fn material_flags(record: &RegistryRecord) -> Option<u32> {
    let (_, _, alpha) = inventory().entry(record)?;
    Some(match alpha {
        ALPHA_CUTOUT => MATERIAL_FLAG_ALPHA_CUTOUT,
        ALPHA_BLEND => MATERIAL_FLAG_ALPHA_BLEND,
        _ => 0,
    })
}

pub(in crate::compiler) fn neutral_material(
    records: &[RegistryRecord],
    pack: &PackSources,
    material_by_descriptor: &BTreeMap<Descriptor, u32>,
) -> Result<u32, AssetError> {
    if !records.iter().any(is_record) {
        return Ok(DIAGNOSTIC_MATERIAL);
    }
    records
        .iter()
        .find(|record| record.name.as_ref() == "minecraft:stone")
        .and_then(|record| descriptor_for(pack, record, BlockFace::Up))
        .and_then(|(descriptor, _)| material_by_descriptor.get(&descriptor).copied())
        .ok_or_else(|| AssetError::InvalidCompiledAssets {
            detail: "missing canonical stone material for vanilla fallback visuals".into(),
        })
}

pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    template_by_key: &mut BTreeMap<CuboidTemplateKey, u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    let Some((min, max, _)) = inventory().entry(record) else {
        return Ok(CompileRuleResult::NoMatch);
    };
    let materials = BlockFace::ALL.map(|face| {
        inputs
            .material(record, face)
            .unwrap_or(inputs.vanilla_fallback_material)
    });

    let (min, max) = if min.iter().zip(max).any(|(min, max)| *min >= max) {
        ([0; 3], [256; 3])
    } else {
        (min, max)
    };
    let mut visual = diagnostic_visual(record);
    let template = intern_cuboid_template(
        materials,
        min,
        max,
        template_by_key,
        storage.templates,
        storage.quads,
    )?;
    set_model_visual(&mut visual, materials, template);
    visual.support = VisualSupport::VanillaFallback;
    Ok(CompileRuleResult::Compiled(visual))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_inventory_is_sorted_unique_and_bounded() {
        let table = FallbackInventory::parse(FALLBACK_BYTES).expect("parse embedded inventory");
        assert_eq!(table.entry_count, 2_031);
        assert!(
            (1..table.entry_count)
                .all(|index| table.entry_at(index - 1).0 < table.entry_at(index).0)
        );
        let zero_volume = (0..table.entry_count)
            .map(|index| table.entry_at(index))
            .filter(|(_, _, min, max, _)| min.iter().zip(max).any(|(min, max)| min >= max))
            .count();
        assert_eq!(zero_volume, 5);
        assert!(
            (0..table.entry_count)
                .map(|index| table.entry_at(index))
                .all(|(_, _, min, max, alpha)| {
                    min.iter().all(|value| (0..=256).contains(value))
                        && max.iter().all(|value| (0..=256).contains(value))
                        && matches!(alpha, ALPHA_CUTOUT | ALPHA_BLEND)
                })
        );
    }

    #[test]
    fn generated_inventory_matches_only_the_pinned_canonical_registry_identities() {
        let records = assets::read_registry(include_bytes!(
            "../../../../assets/data/block-registry-v1001.bin"
        ))
        .expect("decode pinned protocol-1001 registry");
        let matched = records
            .iter()
            .filter(|record| inventory().entry(record).is_some())
            .collect::<Vec<_>>();
        let names = matched
            .iter()
            .map(|record| record.name.as_ref())
            .collect::<std::collections::BTreeSet<_>>();

        assert_eq!(matched.len(), 2_031);
        assert_eq!(names.len(), 335);
        assert!(
            matched
                .iter()
                .all(|record| !record.flags.contains(BlockFlags::AIR))
        );

        let mut tampered = (*matched[0]).clone();
        tampered.canonical_state = format!("{} ", tampered.canonical_state).into_boxed_str();
        assert_eq!(inventory().entry(&tampered), None);
    }

    /// Builds a synthetic fallback-table byte span with an arbitrary entry count.
    fn synthetic_table(entries: &[(u32, &str)]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        bytes.extend_from_slice(&(entries.len() as u32).to_le_bytes());
        for (hash, name) in entries {
            let record = RegistryRecord {
                sequential_id: 0,
                network_hash: *hash,
                name: (*name).into(),
                canonical_state: "{}".into(),
                flags: BlockFlags::empty(),
                model_family: ModelFamily::Unknown,
                contributor_role: ContributorRole::Primary,
                model_state: Default::default(),
                face_coverage: 0,
                collision_seed: Default::default(),
                provenance: assets::RegistryProvenance::PMMP,
            };
            bytes.extend_from_slice(&hash.to_le_bytes());
            bytes.extend_from_slice(&identity_fingerprint(&record).to_le_bytes());
            for value in [[0_i16; 3], [16, 16, 16]] {
                for coordinate in value {
                    bytes.extend_from_slice(&coordinate.to_le_bytes());
                }
            }
            bytes.push(ALPHA_CUTOUT);
        }
        bytes
    }

    fn synthetic_record(name: &str, hash: u32) -> RegistryRecord {
        RegistryRecord {
            sequential_id: 7,
            network_hash: hash,
            name: name.into(),
            canonical_state: "{}".into(),
            flags: BlockFlags::empty(),
            model_family: ModelFamily::Unknown,
            contributor_role: ContributorRole::Primary,
            model_state: Default::default(),
            face_coverage: 0,
            collision_seed: Default::default(),
            provenance: assets::RegistryProvenance::PMMP,
        }
    }

    #[test]
    fn a_sibling_table_with_a_different_header_entry_count_parses_and_resolves_entries() {
        let entries = [(10_u32, "minecraft:a"), (20_u32, "minecraft:b")];
        let bytes = synthetic_table(&entries);
        let table = FallbackInventory::parse(&bytes).expect("parse synthetic sibling table");

        // A different count than the embedded 2,031-entry table proves the
        // header drives the lookup bounds.
        assert_eq!(table.entry_count, 2);
        let hit = table
            .entry(&synthetic_record("minecraft:a", 10))
            .expect("matching identity resolves");
        assert_eq!(hit, ([0, 0, 0], [16, 16, 16], ALPHA_CUTOUT));
        assert_eq!(table.entry(&synthetic_record("minecraft:c", 30)), None);
        // Same hash but a different canonical identity must not resolve.
        assert_eq!(table.entry(&synthetic_record("minecraft:x", 10)), None);
    }

    #[test]
    fn corrupted_synthetic_table_headers_fail_closed() {
        let valid = synthetic_table(&[(10_u32, "minecraft:a")]);

        let mut bad_magic = valid.clone();
        bad_magic[0] = b'X';
        assert!(matches!(
            FallbackInventory::parse(&bad_magic),
            Err(AssetError::InvalidCompiledAssets { .. })
        ));

        let mut bad_version = valid.clone();
        bad_version[8] = 2;
        assert!(matches!(
            FallbackInventory::parse(&bad_version),
            Err(AssetError::InvalidCompiledAssets { .. })
        ));

        let truncated = &valid[..valid.len() - 1];
        assert!(matches!(
            FallbackInventory::parse(truncated),
            Err(AssetError::InvalidCompiledAssets { .. })
        ));

        let mut lying_count = valid.clone();
        lying_count[12..16].copy_from_slice(&3_u32.to_le_bytes());
        assert!(matches!(
            FallbackInventory::parse(&lying_count),
            Err(AssetError::InvalidCompiledAssets { .. })
        ));
    }
}
