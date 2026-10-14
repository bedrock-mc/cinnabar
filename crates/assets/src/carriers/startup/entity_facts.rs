//! Required entity facts are installed beside the other runtime carriers.
use super::AssetStartupError;
use crate::facts::entity::{EntityFacts, MAX_BYTES};
use std::{io::Read, path::Path};

/// Rejects missing, corrupt or stale facts before a client session can start.
pub fn require_entity_facts(world_path: &Path) -> Result<(), AssetStartupError> {
    let path = world_path.with_file_name(crate::carriers::ENTITY_FACTS.output);
    let error = |detail: String| AssetStartupError::EntityFacts {
        path: path.clone(),
        detail,
    };
    let mut bytes = Vec::new();
    std::fs::File::open(&path)
        .map_err(|e| error(e.to_string()))?
        .take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| error(e.to_string()))?;
    EntityFacts::validate_pinned(&bytes).map_err(|e| error(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::entity::SOURCE_BYTES;
    #[test]
    fn startup_requires_the_exact_hash_checked_facts() {
        let dir = tempfile::tempdir().unwrap();
        let world = dir.path().join(crate::carriers::WORLD.output);
        assert!(
            require_entity_facts(&world)
                .unwrap_err()
                .to_string()
                .contains("make assets")
        );
        let path = dir.path().join(crate::carriers::ENTITY_FACTS.output);
        std::fs::write(&path, SOURCE_BYTES).unwrap();
        assert!(require_entity_facts(&world).is_ok());
        let mut bytes = SOURCE_BYTES.to_vec();
        bytes[8] ^= 1;
        std::fs::write(&path, bytes).unwrap();
        assert!(require_entity_facts(&world).is_err());
    }
}
