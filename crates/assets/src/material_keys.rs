//! Terrain texture key index over a compiled carrier's materials, so a session
//! can retexture vanilla blocks from a server pack's `terrain_texture.json`.
//! It also carries the vanilla key-to-image aliases, since installs keep no unpacked pack.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Sidecar wire version; part of the world carrier's prepared identity.
pub const MATERIAL_KEYS_SCHEMA: u32 = 3;
/// Largest sidecar file the runtime reads.
pub const MAX_MATERIAL_KEYS_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    schema: u32,
    materials: u32,
    keys: BTreeMap<String, Vec<u32>>,
    aliases: BTreeMap<String, String>,
    fixed_tints: BTreeMap<String, [u8; 3]>,
}

/// Material ids by the terrain texture key they were compiled from.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MaterialKeys {
    keys: BTreeMap<Box<str>, Box<[u32]>>,
    /// Every vanilla terrain key's variant-zero image path.
    aliases: BTreeMap<Box<str>, Box<str>>,
    fixed_tints: BTreeMap<Box<str>, [u8; 3]>,
}

impl MaterialKeys {
    /// Groups `(material id, texture key)` pairs by key, ids sorted and unique.
    pub fn from_entries<K: AsRef<str>>(entries: impl IntoIterator<Item = (u32, K)>) -> Self {
        let mut grouped = BTreeMap::<Box<str>, Vec<u32>>::new();
        for (material, key) in entries {
            grouped
                .entry(key.as_ref().into())
                .or_default()
                .push(material);
        }
        let keys = grouped
            .into_iter()
            .map(|(key, mut ids)| {
                ids.sort_unstable();
                ids.dedup();
                (key, ids.into_boxed_slice())
            })
            .collect();
        Self {
            keys,
            aliases: BTreeMap::new(),
            fixed_tints: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn with_aliases<K: AsRef<str>, P: AsRef<str>>(
        mut self,
        aliases: impl IntoIterator<Item = (K, P)>,
    ) -> Self {
        self.aliases = aliases
            .into_iter()
            .map(|(key, path)| (key.as_ref().into(), path.as_ref().into()))
            .collect();
        self
    }

    #[must_use]
    pub fn with_fixed_tints<K: AsRef<str>>(
        mut self,
        tints: impl IntoIterator<Item = (K, [u8; 3])>,
    ) -> Self {
        self.fixed_tints = tints
            .into_iter()
            .map(|(key, tint)| (key.as_ref().into(), tint))
            .collect();
        self
    }

    /// Literal atlas multipliers already baked into the base material textures.
    pub fn fixed_tints(&self) -> impl Iterator<Item = (&str, [u8; 3])> {
        self.fixed_tints
            .iter()
            .map(|(key, tint)| (key.as_ref(), *tint))
    }

    /// Serializes the sidecar for a carrier with `material_count` materials.
    #[must_use]
    pub fn to_json(&self, material_count: u32) -> Vec<u8> {
        let wire = Wire {
            schema: MATERIAL_KEYS_SCHEMA,
            materials: material_count,
            keys: self
                .keys
                .iter()
                .map(|(key, ids)| (key.to_string(), ids.to_vec()))
                .collect(),
            aliases: self
                .aliases
                .iter()
                .map(|(key, path)| (key.to_string(), path.to_string()))
                .collect(),
            fixed_tints: self
                .fixed_tints
                .iter()
                .map(|(key, tint)| (key.to_string(), *tint))
                .collect(),
        };
        serde_json::to_vec(&wire).expect("material key sidecar serializes")
    }

    /// Parses a sidecar; `None` unless it matches a carrier of `material_count`
    /// materials exactly.
    #[must_use]
    pub fn from_json(bytes: &[u8], material_count: usize) -> Option<Self> {
        let wire: Wire = serde_json::from_slice(bytes).ok()?;
        if wire.schema != MATERIAL_KEYS_SCHEMA || wire.materials as usize != material_count {
            return None;
        }
        let mut keys = BTreeMap::new();
        for (key, ids) in wire.keys {
            if ids.iter().any(|&id| id as usize >= material_count) {
                return None;
            }
            keys.insert(key.into_boxed_str(), ids.into_boxed_slice());
        }
        let aliases = wire
            .aliases
            .into_iter()
            .map(|(key, path)| (key.into_boxed_str(), path.into_boxed_str()))
            .collect();
        let fixed_tints = wire
            .fixed_tints
            .into_iter()
            .map(|(key, tint)| (key.into_boxed_str(), tint))
            .collect();
        Some(Self {
            keys,
            aliases,
            fixed_tints,
        })
    }

    /// Material ids compiled from `key`; empty when the key is unknown.
    #[must_use]
    pub fn materials(&self, key: &str) -> &[u32] {
        self.keys.get(key).map_or(&[], |ids| ids)
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.keys.keys().map(AsRef::as_ref)
    }

    /// `(terrain key, image path)` for every vanilla catalog entry.
    pub fn aliases(&self) -> impl Iterator<Item = (&str, &str)> {
        self.aliases
            .iter()
            .map(|(key, path)| (key.as_ref(), path.as_ref()))
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::MaterialKeys;

    // The sidecar round-trips and refuses a carrier of a different size.
    #[test]
    fn json_round_trips_and_checks_the_material_count() {
        let keys =
            MaterialKeys::from_entries([(3, "stone"), (1, "stone"), (2, "dirt"), (1, "stone")]);
        assert_eq!(keys.materials("stone"), [1, 3]);
        assert_eq!(keys.materials("missing"), [] as [u32; 0]);
        let json = keys.to_json(4);
        assert_eq!(MaterialKeys::from_json(&json, 4), Some(keys));
        assert_eq!(MaterialKeys::from_json(&json, 5), None);
        assert_eq!(MaterialKeys::from_json(&json, 3), None);
    }

    // Packaged installs resolve vanilla aliases from the sidecar; a pre-alias one is stale.
    #[test]
    fn aliases_round_trip_and_alias_free_sidecars_are_rejected() {
        let keys = MaterialKeys::from_entries([(0, "stone")])
            .with_aliases([("stone", "textures/blocks/stone"), ("unused", "textures/x")])
            .with_fixed_tints([("stone", [20, 80, 30])]);
        let decoded = MaterialKeys::from_json(&keys.to_json(1), 1).unwrap();
        assert_eq!(
            decoded.fixed_tints().collect::<Vec<_>>(),
            [("stone", [20, 80, 30])]
        );
        assert_eq!(
            decoded.aliases().collect::<Vec<_>>(),
            [("stone", "textures/blocks/stone"), ("unused", "textures/x")]
        );
        assert_eq!(
            MaterialKeys::from_json(br#"{"schema":1,"materials":1,"keys":{"stone":[0]}}"#, 1),
            None
        );
    }
}
