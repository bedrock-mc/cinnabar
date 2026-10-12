use assets::{RuntimeEntityAssets, item::glint::ItemGlint};
use protocol::{ItemRegistryEntry, ItemRegistryVersion};

/// Resolves intrinsic shine once for a supported numeric registry declaration.
pub(super) fn resolve(
    entry: &ItemRegistryEntry,
    assets: Option<&RuntimeEntityAssets>,
) -> ItemGlint {
    if matches!(entry.version, ItemRegistryVersion::Unknown(_)) {
        return ItemGlint::Unresolved;
    }
    let compiled = assets
        .and_then(|assets| {
            let key = assets::ItemVisualKey {
                identifier: entry.identifier.as_ref().into(),
                metadata: 0,
            };
            assets
                .item_visuals()
                .binary_search_by(|visual| visual.key.cmp(&key))
                .ok()
                .map(|index| assets.item_visuals()[index].glint)
        })
        .unwrap_or_default();
    if let Some(components) = &entry.components {
        if let Some(enabled) = components.glint {
            return ItemGlint::Session { enabled };
        }
        if components.glint_declared {
            return ItemGlint::Unresolved;
        }
    }
    if entry.component_based {
        ItemGlint::Unresolved
    } else {
        compiled
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// Creates one numeric alias with an explicit or absent intrinsic-shine flag.
    fn entry(id: i32, enabled: Option<bool>) -> ItemRegistryEntry {
        let mut entry = protocol::vanilla_item_registry()[0].clone();
        entry.network_id = id;
        entry.identifier = Arc::from("custom:shared_icon");
        entry.component_based = true;
        entry.components = Some(Arc::new(protocol::ItemComponents {
            glint: enabled,
            ..Default::default()
        }));
        entry
    }

    #[test]
    fn numeric_glint_aliases_refresh_false_missing_and_malformed_values() {
        let mut store = super::super::ItemStateStore::diagnostic();
        assert!(store.apply_registry(protocol::ItemRegistryEvent {
            entries: vec![entry(10_001, Some(true)), entry(10_002, Some(false))].into(),
        }));
        assert_eq!(store.item_glint(10_001).enabled(), Some(true));
        assert_eq!(store.item_glint(10_002).enabled(), Some(false));
        assert!(store.apply_registry(protocol::ItemRegistryEvent {
            entries: vec![entry(10_001, Some(false))].into(),
        }));
        assert_eq!(store.item_glint(10_001).enabled(), Some(false));
        assert_eq!(store.item_glint(10_002), ItemGlint::Unresolved);
        let mut malformed = entry(10_001, None);
        Arc::make_mut(malformed.components.as_mut().unwrap()).glint_declared = true;
        assert!(store.apply_registry(protocol::ItemRegistryEvent {
            entries: vec![malformed].into()
        }));
        assert_eq!(store.item_glint(10_001), ItemGlint::Unresolved);
        let mut unsupported = entry(10_001, Some(true));
        unsupported.version = ItemRegistryVersion::Unknown(42);
        assert_eq!(resolve(&unsupported, None), ItemGlint::Unresolved);
    }
}
