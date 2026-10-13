use std::collections::BTreeMap;

use assets::{AssetError, item::armour::ItemArmour};
use serde::Deserialize;

use super::super::invalid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArmourTable {
    schema: u32,
    protocol: u32,
    items: Box<[ArmourEntry]>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArmourEntry {
    identifier: Box<str>,
    armour: ItemArmour,
}

/// Reads the single protection source once while compiling item definitions.
pub(super) fn reviewed() -> Result<BTreeMap<Box<str>, ItemArmour>, AssetError> {
    let table: ArmourTable = serde_json::from_slice(assets::item::armour::ITEM_ARMOUR_SOURCE_BYTES)
        .map_err(|source| AssetError::Json {
            path: "crates/assets/data/item-armour-v2193.json".into(),
            source,
        })?;
    if table.schema != 1 || table.protocol != super::ROUTE_PROTOCOL {
        return Err(invalid(
            "item armour table version does not match the registry",
        ));
    }
    let mut entries = BTreeMap::new();
    for entry in table.items {
        if !entry.identifier.starts_with("minecraft:")
            || entries.insert(entry.identifier, entry.armour).is_some()
        {
            return Err(invalid(
                "item armour table contains a noncanonical or duplicate identifier",
            ));
        }
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_positive_armour_identifier_is_in_the_independent_retail_inventory() {
        let retail = std::str::from_utf8(assets::item::retail::SOURCE_BYTES).unwrap();
        let identifiers = retail
            .lines()
            .filter_map(|line| line.split('\t').nth(1))
            .collect::<std::collections::BTreeSet<_>>();
        for identifier in reviewed().unwrap().keys() {
            assert!(
                identifiers.contains(identifier.as_ref()),
                "unadmitted armour {identifier}"
            );
        }
    }
}
