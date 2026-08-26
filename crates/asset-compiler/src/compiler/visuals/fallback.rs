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
/// An embedded table can never describe more identities than the bounded
/// registry itself may carry.
const MAX_ENTRY_COUNT: usize = 65_536;

/// One embedded provisional fallback inventory stamped with the exact
/// registry wire protocol whose network hashes key its entries.
struct EmbeddedTable {
    protocol: u32,
    bytes: &'static [u8],
}

/// Every provisional fallback inventory this compiler can consume. A compile
/// resolves exactly the table stamped for the active block registry's
/// header-derived wire protocol; no other table may serve it.
const EMBEDDED_TABLES: [EmbeddedTable; 2] = [
    EmbeddedTable {
        protocol: 1001,
        bytes: include_bytes!("../../../data/vanilla-fallback-v1001.bin"),
    },
    EmbeddedTable {
        protocol: 2168,
        bytes: include_bytes!("../../../../assets/data/vanilla-fallback-v2168.bin"),
    },
];

/// One parse-once slot per embedded table, indexed in lockstep.
const PARSED_SLOTS: usize = EMBEDDED_TABLES.len();
const _: () = assert!(EMBEDDED_TABLES[0].bytes.len() >= HEADER_BYTES);
const _: () = assert!(EMBEDDED_TABLES[1].bytes.len() >= HEADER_BYTES);

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

/// One parsed provisional fallback inventory bound to its validated byte span.
///
/// The entry count comes from the table header instead of a compile-time
/// constant, so a sibling table for another registry protocol (with a
/// different count) parses through the same code path while magic, format
/// version, and exact length stay strictly validated.
#[derive(Debug)]
pub(in crate::compiler) struct FallbackInventory<'a> {
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

    /// Whether the record's network identity carries a provisional fallback
    /// visual in this inventory.
    pub(in crate::compiler) fn contains(&self, record: &RegistryRecord) -> bool {
        self.entry(record).is_some()
    }

    /// Material alpha flags for a record carrying a provisional fallback
    /// visual, or `None` when this inventory does not cover the record.
    pub(in crate::compiler) fn material_flags(&self, record: &RegistryRecord) -> Option<u32> {
        let (_, _, alpha) = self.entry(record)?;
        Some(match alpha {
            ALPHA_CUTOUT => MATERIAL_FLAG_ALPHA_CUTOUT,
            ALPHA_BLEND => MATERIAL_FLAG_ALPHA_BLEND,
            _ => 0,
        })
    }

    /// Every entry failing to resolve against `records`, in table order, each
    /// named with the exact failure reason.
    ///
    /// Like the pinned matched-count gates, coverage enforcement runs at the
    /// checked-in-artifact boundary rather than per compile: synthetic
    /// fixture registries legitimately cover none of the provisional
    /// inventory, while every regenerated artifact pair must join exactly.
    #[cfg(test)]
    fn unresolved_entries(&self, records: &[RegistryRecord]) -> Vec<String> {
        let mut fingerprints = BTreeMap::<u32, Vec<u64>>::new();
        for record in records {
            fingerprints
                .entry(record.network_hash)
                .or_default()
                .push(identity_fingerprint(record));
        }
        let mut unresolved = Vec::new();
        for index in 0..self.entry_count {
            let (hash, fingerprint, _, _, _) = self.entry_at(index);
            match fingerprints.get(&hash) {
                None => unresolved.push(format!("network hash {hash:#010x} has no registry record")),
                Some(candidates) if !candidates.contains(&fingerprint) => unresolved.push(format!(
                    "network hash {hash:#010x} resolves only to records with a different canonical identity"
                )),
                Some(_) => {}
            }
        }
        unresolved
    }

    /// Coverage gate over one decoded registry: fails closed listing every
    /// unresolvable entry, so a stale or foreign-identity inventory can never
    /// silently shrink provisional coverage or launder diagnostic visuals
    /// into claimed-exact ones.
    #[cfg(test)]
    fn validate_coverage(&self, records: &[RegistryRecord]) -> Result<(), AssetError> {
        let unresolved = self.unresolved_entries(records);
        if unresolved.is_empty() {
            return Ok(());
        }
        Err(invalid_fallback_detail(format!(
            "{} vanilla-fallback entries fail to resolve against the registry: {}",
            unresolved.len(),
            unresolved.join("; "),
        )))
    }
}

fn invalid_fallback(detail: &'static str) -> AssetError {
    AssetError::InvalidCompiledAssets {
        detail: detail.into(),
    }
}

fn invalid_fallback_detail(detail: String) -> AssetError {
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

/// Resolves the parsed embedded inventory stamped for the active registry
/// wire protocol. A protocol with no stamped table fails closed with typed
/// attribution naming both the active protocol and every supported stamp.
pub(in crate::compiler) fn inventory(
    protocol: u32,
) -> Result<&'static FallbackInventory<'static>, AssetError> {
    static PARSED: [OnceLock<FallbackInventory<'static>>; PARSED_SLOTS] =
        [OnceLock::new(), OnceLock::new()];
    let Some((slot, table)) = EMBEDDED_TABLES
        .iter()
        .enumerate()
        .find(|(_, table)| table.protocol == protocol)
    else {
        return Err(invalid_fallback_detail(format!(
            "active registry protocol {protocol} has no embedded vanilla-fallback inventory; embedded tables are stamped for protocols {:?}",
            EMBEDDED_TABLES
                .iter()
                .map(|table| table.protocol)
                .collect::<Vec<_>>()
        )));
    };
    Ok(PARSED[slot].get_or_init(|| {
        FallbackInventory::parse(table.bytes)
            .expect("embedded vanilla-fallback inventory must parse")
    }))
}

pub(in crate::compiler) fn neutral_material(
    fallback: &FallbackInventory,
    records: &[RegistryRecord],
    pack: &PackSources,
    material_by_descriptor: &BTreeMap<Descriptor, u32>,
) -> Result<u32, AssetError> {
    if !records.iter().any(|record| fallback.contains(record)) {
        return Ok(DIAGNOSTIC_MATERIAL);
    }
    records
        .iter()
        .find(|record| record.name.as_ref() == "minecraft:stone")
        .and_then(|record| descriptor_for(fallback, pack, record, BlockFace::Up))
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
    let Some((min, max, _)) = inputs.fallback.entry(record) else {
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
    fn embedded_inventories_are_sorted_unique_and_bounded() {
        for table_bytes in EMBEDDED_TABLES.iter().map(|table| table.bytes) {
            let table = FallbackInventory::parse(table_bytes).expect("parse embedded inventory");
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
    }

    #[test]
    fn generated_inventory_matches_only_the_pinned_canonical_registry_identities() {
        let records = assets::read_registry(include_bytes!(
            "../../../../assets/data/block-registry-v1001.bin"
        ))
        .expect("decode pinned protocol-1001 registry");
        let table = inventory(1001).expect("resolve the legacy-stamped inventory");
        let matched = records
            .iter()
            .filter(|record| table.contains(record))
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
        assert_eq!(table.entry(&tampered), None);
    }

    #[test]
    fn generated_current_inventory_matches_only_the_checked_in_current_registry_identities() {
        let records = assets::read_registry_for_protocol(
            include_bytes!("../../../../assets/data/block-registry-v2168.bin"),
            2168,
        )
        .expect("decode checked-in protocol-2168 registry");
        let table = inventory(2168).expect("resolve the current-stamped inventory");
        let matched = records
            .iter()
            .filter(|record| table.contains(record))
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
        assert_eq!(table.entry(&tampered), None);
    }

    #[test]
    fn every_embedded_entry_resolves_against_its_own_stamped_registry() {
        let legacy_records = assets::read_registry(include_bytes!(
            "../../../../assets/data/block-registry-v1001.bin"
        ))
        .expect("decode pinned protocol-1001 registry");
        inventory(1001)
            .expect("resolve the legacy-stamped inventory")
            .validate_coverage(&legacy_records)
            .expect("every legacy entry resolves against the legacy registry");

        let current_records = assets::read_registry_for_protocol(
            include_bytes!("../../../../assets/data/block-registry-v2168.bin"),
            2168,
        )
        .expect("decode checked-in protocol-2168 registry");
        inventory(2168)
            .expect("resolve the current-stamped inventory")
            .validate_coverage(&current_records)
            .expect("every current entry resolves against the current registry");
    }

    #[test]
    fn selection_binds_each_supported_protocol_to_its_own_stamped_inventory() {
        let legacy = inventory(1001).expect("resolve the legacy-stamped inventory");
        let current = inventory(2168).expect("resolve the current-stamped inventory");
        assert!(!std::ptr::eq(legacy, current));

        let error = inventory(9999).expect_err("an unstamped protocol must fail closed");
        let message = error.to_string();
        assert!(message.contains("9999"), "{message}");
        assert!(message.contains("1001"), "{message}");
        assert!(message.contains("2168"), "{message}");
    }

    #[test]
    fn coverage_validation_lists_unresolvable_entries_in_both_identity_directions() {
        let table_bytes = synthetic_table(&[(10_u32, "minecraft:a"), (20_u32, "minecraft:b")]);
        let table = FallbackInventory::parse(&table_bytes).expect("parse synthetic table");

        // One direction: the hash survives but only under a diverged
        // identity, and the remaining hash is entirely absent.
        let mut drifted = synthetic_record("minecraft:a", 10);
        drifted.canonical_state = r#"{"drifted":true}"#.into();
        let error = table
            .validate_coverage(&[drifted])
            .expect_err("diverged and absent identities must fail coverage");
        let message = error.to_string();
        assert!(message.contains("2 vanilla-fallback entries"), "{message}");
        assert!(message.contains("0x0000000a"), "{message}");
        assert!(
            message.contains("different canonical identity"),
            "{message}"
        );
        assert!(message.contains("0x00000014"), "{message}");
        assert!(message.contains("has no registry record"), "{message}");

        // The reverse direction: the registry carries neither identity.
        let error = table
            .validate_coverage(&[synthetic_record("minecraft:c", 30)])
            .expect_err("fully absent identities must fail coverage");
        let message = error.to_string();
        assert!(message.contains("2 vanilla-fallback entries"), "{message}");
        assert!(message.contains("0x0000000a"), "{message}");
        assert!(message.contains("0x00000014"), "{message}");
    }

    #[test]
    fn a_stale_table_copy_still_parses_but_fails_coverage() {
        let mut stale = EMBEDDED_TABLES[1].bytes.to_vec();
        stale[HEADER_BYTES + 4] ^= 1;
        let table = FallbackInventory::parse(&stale).expect("a stale copy keeps a valid format");
        let records = assets::read_registry_for_protocol(
            include_bytes!("../../../../assets/data/block-registry-v2168.bin"),
            2168,
        )
        .expect("decode checked-in protocol-2168 registry");

        let error = table
            .validate_coverage(&records)
            .expect_err("a tampered identity must fail coverage");
        assert!(
            error.to_string().contains("different canonical identity"),
            "{error}"
        );
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

    /// Parses an empty bounded pack fixture so rule-path witnesses exercise
    /// the same descriptor resolution as production compiles.
    fn empty_pack_fixture() -> PackSources {
        let directory = tempfile::tempdir().expect("create fallback fixture pack");
        let root = directory.path().to_path_buf();
        // `read_pack` borrows nothing from the directory after parsing, so
        // leak the temporary directory for the lifetime of the sources.
        std::mem::forget(directory);
        std::fs::create_dir_all(root.join("textures")).expect("create fixture texture directory");
        std::fs::write(root.join("blocks.json"), "{}").expect("write blocks fixture");
        std::fs::write(
            root.join("textures/terrain_texture.json"),
            r#"{"texture_data":{}}"#,
        )
        .expect("write terrain fixture");
        std::fs::write(root.join("textures/flipbook_textures.json"), "[]")
            .expect("write flipbook fixture");
        read_pack(&root).expect("parse fallback fixture pack")
    }

    /// Discriminating witness: two protocol-bound tables differing for one
    /// identity drive opposite rule outcomes for the same record, so the
    /// rule path provably consumes whichever table its protocol stamp bound.
    #[test]
    fn a_compilation_consumes_exactly_the_table_bound_for_its_protocol() {
        let pack = empty_pack_fixture();
        let covered_bytes = synthetic_table(&[(10_u32, "minecraft:a")]);
        let covered: &'static FallbackInventory<'static> = Box::leak(Box::new(
            FallbackInventory::parse(Box::leak(covered_bytes.into_boxed_slice()))
                .expect("parse covered table"),
        ));
        let empty_bytes = synthetic_table(&[]);
        let empty: &'static FallbackInventory<'static> = Box::leak(Box::new(
            FallbackInventory::parse(Box::leak(empty_bytes.into_boxed_slice()))
                .expect("parse empty table"),
        ));
        let record = synthetic_record("minecraft:a", 10);

        let compile_with = |fallback| {
            let mut templates = Vec::new();
            let mut quads = Vec::new();
            let inputs = RuleInputs {
                pack: &pack,
                material_by_descriptor: &BTreeMap::new(),
                vanilla_fallback_material: DIAGNOSTIC_MATERIAL,
                fallback,
            };
            compile_rule(
                &record,
                &inputs,
                &mut BTreeMap::new(),
                &mut ModelStorage {
                    templates: &mut templates,
                    quads: &mut quads,
                },
            )
            .expect("run the fallback rule")
        };

        let CompileRuleResult::Compiled(visual) = compile_with(covered) else {
            panic!("a covered identity must compile through its bound table");
        };
        assert_eq!(visual.support, VisualSupport::VanillaFallback);
        assert_eq!(compile_with(empty), CompileRuleResult::NoMatch);
    }
}
