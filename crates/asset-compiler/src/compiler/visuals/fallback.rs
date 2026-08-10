use super::super::*;
use super::context::{
    CuboidTemplateKey, ModelStorage, RuleInputs, diagnostic_visual, intern_cuboid_template,
    set_model_visual,
};
use super::dispatcher::CompileRuleResult;
const ALPHA_CUTOUT: u8 = 1;
const ALPHA_BLEND: u8 = 2;
const HEADER_BYTES: usize = 16;
const ENTRY_BYTES: usize = 25;
const ENTRY_COUNT: usize = 2_031;
const FALLBACK_BYTES: &[u8] = include_bytes!("../../../data/vanilla-fallback-v1001.bin");
const _: () = assert!(FALLBACK_BYTES.len() == HEADER_BYTES + ENTRY_COUNT * ENTRY_BYTES);

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

fn u16_at(offset: usize) -> u16 {
    u16::from_le_bytes([FALLBACK_BYTES[offset], FALLBACK_BYTES[offset + 1]])
}

fn u32_at(offset: usize) -> u32 {
    u32::from_le_bytes([
        FALLBACK_BYTES[offset],
        FALLBACK_BYTES[offset + 1],
        FALLBACK_BYTES[offset + 2],
        FALLBACK_BYTES[offset + 3],
    ])
}

fn u64_at(offset: usize) -> u64 {
    u64::from_le_bytes([
        FALLBACK_BYTES[offset],
        FALLBACK_BYTES[offset + 1],
        FALLBACK_BYTES[offset + 2],
        FALLBACK_BYTES[offset + 3],
        FALLBACK_BYTES[offset + 4],
        FALLBACK_BYTES[offset + 5],
        FALLBACK_BYTES[offset + 6],
        FALLBACK_BYTES[offset + 7],
    ])
}

fn entry_at(index: usize) -> (u32, u64, [i16; 3], [i16; 3], u8) {
    let offset = HEADER_BYTES + index * ENTRY_BYTES;
    let coordinate = |relative| u16_at(offset + relative) as i16;
    (
        u32_at(offset),
        u64_at(offset + 4),
        [coordinate(12), coordinate(14), coordinate(16)],
        [coordinate(18), coordinate(20), coordinate(22)],
        FALLBACK_BYTES[offset + 24],
    )
}

fn entry(record: &RegistryRecord) -> Option<([i16; 3], [i16; 3], u8)> {
    let mut left = 0;
    let mut right = ENTRY_COUNT;
    while left < right {
        let middle = left + (right - left) / 2;
        match entry_at(middle).0.cmp(&record.network_hash) {
            std::cmp::Ordering::Less => left = middle + 1,
            std::cmp::Ordering::Greater => right = middle,
            std::cmp::Ordering::Equal => {
                let (_, fingerprint, min, max, alpha) = entry_at(middle);
                return (fingerprint == identity_fingerprint(record)).then_some((min, max, alpha));
            }
        }
    }
    None
}

pub(in crate::compiler) fn is_record(record: &RegistryRecord) -> bool {
    entry(record).is_some()
}

pub(in crate::compiler) fn material_flags(record: &RegistryRecord) -> Option<u32> {
    let (_, _, alpha) = entry(record)?;
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
    let Some((min, max, _)) = entry(record) else {
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
        assert_eq!(&FALLBACK_BYTES[..8], b"CVFB1001");
        assert_eq!(u32_at(8), 1);
        assert_eq!(u32_at(12) as usize, ENTRY_COUNT);
        assert!((1..ENTRY_COUNT).all(|index| entry_at(index - 1).0 < entry_at(index).0));
        let zero_volume = (0..ENTRY_COUNT)
            .map(entry_at)
            .filter(|(_, _, min, max, _)| min.iter().zip(max).any(|(min, max)| min >= max))
            .count();
        assert_eq!(zero_volume, 5);
        assert!(
            (0..ENTRY_COUNT)
                .map(entry_at)
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
            .filter(|record| entry(record).is_some())
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
        assert_eq!(entry(&tampered), None);
    }
}
