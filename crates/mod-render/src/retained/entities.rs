//! Host-issued actor references; guest tokens never contain a reusable runtime ID alone.
use super::*;

/// Exact actor incarnation in the world authority's namespace.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct EntityKey {
    pub session: u64,
    pub dimension: i32,
    pub runtime_id: u64,
    pub spawn_revision: u64,
}

impl Store {
    /// Reconciles confirmed despawns immediately; temporary invisibility keeps references.
    pub fn reconcile_entities(
        &mut self,
        mut candidates: Vec<EntityKey>,
        despawned: &[EntityKey],
    ) -> Result<(), DrawError> {
        if candidates.len() > mod_api::MAX_DRAW_ENTITIES {
            return Err(error(ErrorCode::BudgetExceeded, "entity observations"));
        }
        candidates.sort_unstable();
        candidates.dedup();
        self.entity_candidates = candidates;
        if despawned.is_empty() {
            return Ok(());
        }
        let mut resources = (*self.committed).clone();
        let removed: Vec<_> = resources
            .entities
            .iter()
            .filter_map(|(handle, key)| despawned.contains(key).then_some(*handle))
            .collect();
        resources
            .entities
            .retain(|handle, _| !removed.contains(handle));
        resources.instances.retain(|_, instance| !matches!(instance.anchor, Anchor::Entity(anchor) if removed.contains(&anchor.entity)));
        if resources != *self.committed {
            resources.instance_revision += 1;
            self.committed = Arc::new(resources);
        }
        self.staged = None;
        Ok(())
    }

    /// Issues owner-scoped references to the bounded current observation set atomically.
    pub fn observe_entities(&mut self) -> Result<Vec<(Handle, EntityKey)>, DrawError> {
        let current = self.base();
        let mut result = Vec::new();
        for key in self.entity_candidates.clone() {
            let handle = if let Some((handle, _)) =
                current.entities.iter().find(|(_, known)| **known == key)
            {
                *handle
            } else {
                self.token()?
            };
            result.push((handle, key));
        }
        self.edit(
            result.len() * std::mem::size_of::<EntityKey>(),
            |resources| {
                let previous = resources.entities.len();
                for (handle, key) in &result {
                    resources.entities.insert(*handle, *key);
                }
                if resources.entities.len() > mod_api::MAX_DRAW_ENTITIES {
                    return Err(error(ErrorCode::BudgetExceeded, "entity handles"));
                }
                if resources.entities.len() != previous {
                    resources.instance_revision += 1;
                }
                Ok(())
            },
        )?;
        Ok(result)
    }
}
