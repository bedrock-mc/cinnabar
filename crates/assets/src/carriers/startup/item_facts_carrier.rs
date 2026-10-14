//! Required gameplay item facts loaded beside the artwork carriers.

use super::AssetStartupError;
use crate::facts::{ItemFacts, MAX_ITEM_FACTS_BYTES};
use std::{fs::File, io::Read, path::Path, sync::Arc};

/// Requires the exact pinned item carrier and returns the shared validated facts.
pub fn require_item_facts(world_asset_path: &Path) -> Result<Arc<ItemFacts>, AssetStartupError> {
    let path = world_asset_path.with_file_name(crate::carriers::ITEM_FACTS.output);
    let load = || -> Result<Arc<ItemFacts>, String> {
        let file = File::open(&path).map_err(|error| error.to_string())?;
        let mut bytes = Vec::new();
        file.take(MAX_ITEM_FACTS_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        ItemFacts::validate_pinned(&bytes).map_err(|error| error.to_string())?;
        ItemFacts::builtin()
            .map(Arc::clone)
            .map_err(|error| error.to_string())
    };
    load().map_err(|detail| AssetStartupError::ItemFacts { path, detail })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_fixture_installs_the_exact_pinned_item_facts() {
        let dir = tempfile::tempdir().unwrap();
        let world = dir.path().join(crate::carriers::WORLD.output);
        super::super::test_carriers::write_pinned_item_facts(&world);
        let facts = require_item_facts(&world).unwrap();
        assert!(Arc::ptr_eq(&facts, ItemFacts::builtin().unwrap()));
    }

    #[test]
    fn missing_corrupt_and_stale_item_facts_fail_startup_with_a_rebuild_command() {
        let dir = tempfile::tempdir().unwrap();
        let world = dir.path().join(crate::carriers::WORLD.output);
        let item = dir.path().join(crate::carriers::ITEM_FACTS.output);
        let error = require_item_facts(&world).unwrap_err().to_string();
        assert!(error.contains(&item.display().to_string()));
        assert!(error.contains("make assets"));
        let pinned = include_bytes!("../../../data/item-facts-v2193.bin");
        std::fs::write(&item, pinned).unwrap();
        assert!(require_item_facts(&world).is_ok());
        let mut corrupted = pinned.to_vec();
        corrupted[20] ^= 1;
        std::fs::write(&item, &corrupted).unwrap();
        assert!(require_item_facts(&world).is_err());
        let mut payload = ItemFacts::builtin().unwrap().payload().clone();
        payload.records[0].fact.allow_off_hand = Some(true);
        std::fs::write(&item, crate::facts::encode_item_facts(payload).unwrap()).unwrap();
        assert!(require_item_facts(&world).is_err());
    }
}
