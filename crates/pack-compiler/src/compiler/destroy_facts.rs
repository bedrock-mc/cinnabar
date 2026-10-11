//! Numeric destroy admission bound to the independent pinned state inventory.

use std::collections::HashMap;

use assets::{BlockVisual, RegistryRecord, compiled::block_destroy::builtin_destroy_facts};

/// Installs facts only for exact pinned identities; every other state stays unresolved.
pub(super) fn install(
    records: &[RegistryRecord],
    visuals: &mut [BlockVisual],
    protocol: u32,
) -> Result<(), assets::AssetError> {
    if protocol != assets::active_content_registry_protocol() {
        for visual in visuals {
            visual.destroy = Default::default();
        }
        return Ok(());
    }
    let pinned = assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )?;
    let inventory: HashMap<_, _> = pinned
        .iter()
        .map(|record| (record.network_hash, record))
        .collect();
    for record in records {
        let Some(visual) = visuals.get_mut(record.sequential_id as usize) else {
            return Err(assets::AssetError::InvalidCompiledAssets {
                detail: "destroy state has no compiled visual".into(),
            });
        };
        visual.destroy = inventory
            .get(&record.network_hash)
            .filter(|bound| {
                bound.name == record.name && bound.canonical_state == record.canonical_state
            })
            .map_or_else(Default::default, |_| builtin_destroy_facts(&record.name));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use assets::{BlockFlags, ContributorRole, compiled::block_destroy::BlockDestroyFacts};

    #[test]
    fn independent_states_reject_changed_names_states_and_hashes() {
        let pinned = assets::read_registry_for_protocol(
            assets::pinned_block_registry_bytes(),
            assets::active_content_registry_protocol(),
        )
        .unwrap();
        let mut visuals =
            vec![
                BlockVisual::diagnostic(BlockFlags::empty(), ContributorRole::Primary);
                pinned
                    .iter()
                    .map(|record| record.sequential_id as usize + 1)
                    .max()
                    .unwrap()
            ];
        install(
            &pinned,
            &mut visuals,
            assets::active_content_registry_protocol(),
        )
        .unwrap();
        let stone = pinned
            .iter()
            .find(|record| record.name.as_ref() == "minecraft:stone")
            .unwrap();
        assert!(matches!(
            visuals[stone.sequential_id as usize].destroy,
            BlockDestroyFacts::Provisional(_)
        ));
        let bedrock = pinned
            .iter()
            .find(|record| record.name.as_ref() == "minecraft:bedrock")
            .unwrap();
        assert_eq!(
            visuals[bedrock.sequential_id as usize].destroy,
            BlockDestroyFacts::ProvisionalIndestructible
        );
        for change in 0..3 {
            let mut changed = stone.clone();
            match change {
                0 => changed.name = "custom:stone".into(),
                1 => changed.canonical_state = "{\"new_state\":1}".into(),
                _ => changed.network_hash ^= 0x8000_0000,
            }
            install(
                &[changed],
                &mut visuals,
                assets::active_content_registry_protocol(),
            )
            .unwrap();
            assert_eq!(
                visuals[stone.sequential_id as usize].destroy,
                BlockDestroyFacts::Unresolved
            );
        }
    }
}
