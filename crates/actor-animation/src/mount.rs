//! Equipment capabilities resolved when an actor or its catalog is admitted.

use super::*;
use assets::entity::mount::MountEquipment;

impl ActorAnimationStore {
    /// Returns cached equipment facts without looking up entity names during UI publication.
    pub fn mount_equipment(&self, runtime_id: u64) -> MountEquipment {
        self.mount_equipment
            .get(&runtime_id)
            .copied()
            .unwrap_or_default()
    }

    /// Resolves a unique entity definition, allowing an unresolved pack entry to replace a built-in.
    pub(super) fn resolve_mount_equipment(&self, actor: &dyn AnimatedActor) -> MountEquipment {
        let ActorKind::Entity { identifier } = actor.kind() else {
            return MountEquipment::Unresolved;
        };
        let find = |assets: &RuntimeEntityAssets| {
            let mut matches = assets.symbols().iter().filter(|symbol| {
                symbol.kind == assets::EntityAssetKind::Entity
                    && symbol.identifier.as_ref() == identifier.as_ref()
            });
            let first = matches.next()?;
            Some(if matches.next().is_some() {
                MountEquipment::Unresolved
            } else {
                first.mount_equipment
            })
        };
        self.pack
            .as_ref()
            .and_then(|pack| find(&pack.assets))
            .or_else(|| self.assets.as_deref().and_then(find))
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assets::entity::mount::{
        MOUNT_EQUIPMENT_SOURCE_BYTES, MOUNT_EQUIPMENT_SOURCE_PATH, builtin_mount_equipment,
    };
    use sha2::{Digest, Sha256};

    /// Uses the existing original rig fixture with a sorted, hash-bound equipment source.
    fn catalog(equipment: MountEquipment) -> Arc<RuntimeEntityAssets> {
        let mut compiled = crate::test_support::counting_random_compiled("minecraft:llama");
        let mut sources = compiled.sources.into_vec();
        sources.insert(
            3,
            assets::EntityAssetSource {
                path: MOUNT_EQUIPMENT_SOURCE_PATH.into(),
                source_bytes: MOUNT_EQUIPMENT_SOURCE_BYTES.len() as u32,
                source_sha256: Sha256::digest(MOUNT_EQUIPMENT_SOURCE_BYTES).into(),
            },
        );
        compiled.sources = sources.into_boxed_slice();
        for symbol in &mut compiled.symbols {
            if symbol.source_index >= 3 {
                symbol.source_index += 1;
            }
        }
        for candidate in &mut compiled.render.candidates {
            if candidate.source >= 3 {
                candidate.source += 1;
            }
        }
        compiled.symbols[0].mount_equipment = equipment;
        Arc::new(RuntimeEntityAssets::from_compiled(compiled).unwrap())
    }

    #[test]
    fn admitted_equipment_is_cached_and_replaced_without_a_frame_name_lookup() {
        let equipment = builtin_mount_equipment("minecraft:llama");
        let mut store = ActorAnimationStore::with_assets(catalog(equipment));
        let mut actor = crate::tests::actor_with_metadata(HashMap::new());
        actor.kind = ActorKind::Entity {
            identifier: "minecraft:llama".into(),
        };
        store.insert(1, 0, &actor);
        assert_eq!(store.mount_equipment(actor.runtime_id), equipment);
        store.set_pack(Some((catalog(MountEquipment::Unresolved), Vec::new())));
        store.insert(1, 0, &actor);
        assert_eq!(
            store.mount_equipment(actor.runtime_id),
            MountEquipment::Unresolved
        );
        store.set_pack(None);
        store.insert(1, 0, &actor);
        assert_eq!(store.mount_equipment(actor.runtime_id), equipment);
        store.remove_runtime(actor.runtime_id);
        assert_eq!(
            store.mount_equipment(actor.runtime_id),
            MountEquipment::Unresolved
        );
        actor.kind = ActorKind::Entity {
            identifier: "fixture:unknown_mount".into(),
        };
        store.insert(1, 0, &actor);
        assert_eq!(
            store.mount_equipment(actor.runtime_id),
            MountEquipment::Unresolved
        );
    }
}
