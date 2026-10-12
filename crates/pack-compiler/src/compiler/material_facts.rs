use std::collections::BTreeMap;

use assets::native_material::NativeMaterialFacts;
use assets::{AssetError, BlockVisual, RegistryRecord};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    format_version: u32,
    materials: BTreeMap<Box<str>, NativeMaterialFacts>,
}

/// Writes each record's material facts onto its visual.
pub(super) fn install(
    records: &[RegistryRecord],
    visuals: &mut [BlockVisual],
    protocol: u32,
) -> Result<(), AssetError> {
    for (record, facts) in records.iter().zip(compile(records, protocol)?) {
        visuals[record.sequential_id as usize].native_material = facts;
    }
    Ok(())
}

/// Joins the partial material catalog to hash-checked pinned state identities.
fn compile(
    records: &[RegistryRecord],
    protocol: u32,
) -> Result<Vec<NativeMaterialFacts>, AssetError> {
    if protocol != assets::active_content_registry_protocol() {
        return Ok(vec![NativeMaterialFacts::Unresolved; records.len()]);
    }
    let source: Source =
        serde_json::from_slice(assets::native_material::NATIVE_MATERIAL_SOURCE_BYTES)
            .map_err(|error| invalid(error.to_string()))?;
    if source.format_version != 1 {
        return Err(invalid("unsupported native material source"));
    }
    let pinned =
        assets::read_registry_for_protocol(assets::pinned_block_registry_bytes(), protocol)?;
    for name in source.materials.keys() {
        if !pinned.iter().any(|record| &record.name == name) {
            return Err(invalid(format!(
                "native material identity {name} is absent from the pinned inventory"
            )));
        }
    }
    let identities: BTreeMap<_, _> = pinned
        .iter()
        .map(|record| (record.network_hash, record))
        .collect();
    Ok(records
        .iter()
        .map(|record| {
            identities
                .get(&record.network_hash)
                .filter(|pinned| {
                    pinned.name == record.name && pinned.canonical_state == record.canonical_state
                })
                .and_then(|pinned| source.materials.get(&pinned.name))
                .copied()
                .unwrap_or_default()
        })
        .collect())
}

/// Reports malformed catalog input through the compiler's checked asset boundary.
fn invalid(detail: impl Into<Box<str>>) -> AssetError {
    AssetError::InvalidCompiledAssets {
        detail: detail.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn material_facts_cover_the_independent_state_inventory_and_reject_false_identities() {
        let protocol = assets::active_content_registry_protocol();
        let records =
            assets::read_registry_for_protocol(assets::pinned_block_registry_bytes(), protocol)
                .unwrap();
        let facts = compile(&records, protocol).unwrap();
        assert_eq!(facts.len(), records.len());
        let plant = records
            .iter()
            .position(|record| record.name.as_ref() == "minecraft:azure_bluet")
            .unwrap();
        assert_eq!(facts[plant], NativeMaterialFacts::NonSolidNonLiquid);
        let stone = records
            .iter()
            .position(|record| record.name.as_ref() == "minecraft:stone")
            .unwrap();
        assert_eq!(facts[stone], NativeMaterialFacts::Unresolved);
        let source: Source =
            serde_json::from_slice(assets::native_material::NATIVE_MATERIAL_SOURCE_BYTES).unwrap();
        for name in source.materials.keys() {
            let states: Vec<_> = records
                .iter()
                .enumerate()
                .filter(|(_, record)| &record.name == name)
                .collect();
            assert!(
                !states.is_empty(),
                "material source has no independently inventoried states: {name}"
            );
            for (index, _) in states {
                assert_ne!(facts[index], NativeMaterialFacts::Unresolved);
            }
        }
        let mut forged = records[plant].clone();
        forged.network_hash = records[stone].network_hash;
        assert_eq!(
            compile(&[forged], protocol).unwrap(),
            [NativeMaterialFacts::Unresolved]
        );
        let mut custom = records[plant].clone();
        custom.name = "fixture:azure_bluet".into();
        assert_eq!(
            compile(&[custom], protocol).unwrap(),
            [NativeMaterialFacts::Unresolved]
        );
        let mut changed = records[plant].clone();
        changed.canonical_state = "{\"unknown\":true}".into();
        assert_eq!(
            compile(&[changed], protocol).unwrap(),
            [NativeMaterialFacts::Unresolved]
        );
    }
}
