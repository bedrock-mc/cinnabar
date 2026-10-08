use std::sync::Arc;

use super::MAX_ACTOR_IDENTIFIER_BYTES;
use crate::nbt_tree::{Nbt, read_root};

/// Maximum retained actor definitions from the session's advertised identifier registry.
pub const MAX_ACTOR_IDENTIFIERS: usize = 4096;

/// A custom identifier may select a native actor constructor through its base identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorIdentifier {
    pub identifier: Arc<str>,
    pub base_identifier: Arc<str>,
}

/// Valid registry entries survive odd fields and unrelated unusable entries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActorIdentifierRegistry {
    pub entries: Arc<[ActorIdentifier]>,
    pub skipped: u64,
}

/// Empty or mistyped base identifiers select vanilla's generic mob constructor.
pub(crate) fn normalize(bytes: &[u8]) -> ActorIdentifierRegistry {
    let Some(root) = read_root(bytes) else {
        return ActorIdentifierRegistry {
            skipped: 1,
            ..Default::default()
        };
    };
    let mut registry = ActorIdentifierRegistry::default();
    let mut entries = Vec::new();
    if !matches!(root.field("idlist"), Some(Nbt::List(_))) {
        registry.skipped = 1;
        return registry;
    }
    for entry in root.list("idlist") {
        let Some(identifier) = entry
            .field("id")
            .and_then(Nbt::as_str)
            .filter(|id| !id.is_empty() && id.len() <= MAX_ACTOR_IDENTIFIER_BYTES)
        else {
            registry.skipped += 1;
            continue;
        };
        if entries.len() == MAX_ACTOR_IDENTIFIERS {
            registry.skipped += 1;
            continue;
        }
        let base = entry.field("bid").and_then(Nbt::as_str).unwrap_or_default();
        if base.len() > MAX_ACTOR_IDENTIFIER_BYTES {
            registry.skipped += 1;
            continue;
        }
        entries.push(ActorIdentifier {
            identifier: canonical_identifier(identifier),
            base_identifier: canonical_identifier(base),
        });
    }
    registry.entries = entries.into();
    registry
}

/// Unqualified actor names use the vanilla namespace and omit the spawn-event suffix.
fn canonical_identifier(identifier: &str) -> Arc<str> {
    let identifier = identifier.split('<').next().unwrap_or_default();
    if identifier.is_empty() || identifier.contains(':') {
        Arc::from(identifier)
    } else {
        Arc::from(format!("minecraft:{identifier}"))
    }
}

#[cfg(test)]
mod tests {
    use super::normalize;

    /// Encodes one string field in network NBT for the registry fixture.
    fn string(bytes: &mut Vec<u8>, key: &str, value: &str) {
        bytes.extend([8, key.len() as u8]);
        bytes.extend(key.as_bytes());
        bytes.push(value.len() as u8);
        bytes.extend(value.as_bytes());
    }

    #[test]
    fn registry_keeps_valid_entries_and_counts_odd_identifiers() {
        let mut bytes = vec![10, 0, 9, 6];
        bytes.extend(b"idlist");
        bytes.extend([10, 6]);
        string(&mut bytes, "id", "custom:arrow");
        string(&mut bytes, "bid", "minecraft:arrow");
        bytes.push(0);
        string(&mut bytes, "id", "custom:mob");
        bytes.push(0);
        string(&mut bytes, "id", "");
        bytes.extend([0, 0]);
        let registry = normalize(&bytes);
        assert_eq!(registry.skipped, 1);
        assert_eq!(registry.entries.len(), 2);
        assert_eq!(&*registry.entries[0].base_identifier, "minecraft:arrow");
        assert!(registry.entries[1].base_identifier.is_empty());
    }
}
