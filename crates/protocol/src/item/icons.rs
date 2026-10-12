//! Item icon texture keys named by StartGame item components.

use std::sync::Arc;

use bedrock_protocol_1_26_51::packets::ItemRegistry as ItemRegistryPacket;

use crate::nbt_tree::read_root;

const MAX_ICON_KEY_BYTES: usize = 256;

/// Returns `(identifier, item_texture key)` for StartGame items: the icon the
/// components name, else the identifier's short name, which vanilla item
/// textures are keyed by for most items (a pack only overrides when it defines
/// that key). Malformed or oversized components fall back to the short name.
#[must_use]
pub(crate) fn item_icon_keys(registry: &ItemRegistryPacket) -> Box<[(Arc<str>, Arc<str>)]> {
    registry
        .item_data
        .iter()
        .filter_map(|item| {
            let named = super::encode_extra(&item.item_component_data)
                .ok()
                .and_then(|bytes| icon_key(&bytes));
            let identifier = item.item_name.to_string_lossy();
            let key = named.or_else(|| short_name_key(&identifier))?;
            Some((Arc::from(identifier), key))
        })
        .collect()
}

fn short_name_key(identifier: &str) -> Option<Arc<str>> {
    let name = identifier
        .rsplit_once(':')
        .map_or(identifier, |(_, name)| name);
    (!name.is_empty() && name.len() <= MAX_ICON_KEY_BYTES).then(|| name.into())
}

/// Reads `components["minecraft:icon"]` (or the legacy `item_properties` copy): a key, or an object
/// naming it under `textures.default` or `texture`.
pub(super) fn icon_key(bytes: &[u8]) -> Option<Arc<str>> {
    let root = read_root(bytes)?;
    let components = root.field("components")?;
    let icon = components
        .field("minecraft:icon")
        .or_else(|| components.field("item_properties")?.field("minecraft:icon"))?;
    let key = icon
        .as_str()
        .or_else(|| icon.field("textures")?.field("default")?.as_str())
        .or_else(|| icon.field("texture")?.as_str())?;
    (!key.is_empty() && key.len() <= MAX_ICON_KEY_BYTES).then(|| key.into())
}

#[cfg(test)]
mod tests {
    use super::icon_key;

    fn named(tag: u8, name: &str) -> Vec<u8> {
        let mut bytes = vec![tag, name.len() as u8];
        bytes.extend_from_slice(name.as_bytes());
        bytes
    }

    fn component_nbt(icon: &[u8]) -> Vec<u8> {
        let mut nbt = named(10, "");
        nbt.extend(named(10, "components"));
        nbt.extend(named(10, "item_properties"));
        nbt.extend_from_slice(icon);
        nbt.extend([0, 0, 0]);
        nbt
    }

    #[test]
    fn icon_key_reads_current_and_legacy_shapes() {
        let mut current = named(10, "minecraft:icon");
        current.extend(named(10, "textures"));
        current.extend(named(8, "default"));
        current.extend([9]);
        current.extend(b"test:tool");
        current.extend([0, 0]);
        assert_eq!(
            icon_key(&component_nbt(&current)).as_deref(),
            Some("test:tool")
        );

        let mut legacy = named(8, "minecraft:icon");
        legacy.extend([4]);
        legacy.extend(b"gem1");
        assert_eq!(icon_key(&component_nbt(&legacy)).as_deref(), Some("gem1"));
        assert!(icon_key(&component_nbt(&[])).is_none());

        // The current layout puts the icon beside item_properties, not inside it.
        let mut direct = named(10, "components");
        direct.extend(named(8, "minecraft:icon"));
        direct.extend([5]);
        direct.extend(b"blade");
        direct.extend([0]);
        let mut nbt = named(10, "");
        nbt.extend(direct);
        nbt.extend([0]);
        assert_eq!(icon_key(&nbt).as_deref(), Some("blade"));
    }
}
