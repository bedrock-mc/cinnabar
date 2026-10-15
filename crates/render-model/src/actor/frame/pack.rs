//! Publishes a session's entity and equipment ranges, staging both when they exceed the
//! catalog budget and admitting staged rigs as frames draw them.

use std::{
    collections::{BTreeSet, HashMap},
    hash::{DefaultHasher, Hash, Hasher},
};

use super::*;
use crate::ActorRigFrameBuilder;

/// Server-pack geometry held outside the catalog while the pack exceeds its budget.
#[derive(Debug, Default)]
pub(super) struct StagedPack {
    /// Every pack-range geometry while staged; empty when the whole pack is resident.
    pub(super) geometries: BTreeMap<EntityRigId, ActorRigGeometry>,
    /// Frame generation each resident staged rig was last drawn in.
    last_drawn: HashMap<EntityRigId, u64>,
    /// Catalog revision and drawn set of the last admission that left a rig out.
    denied: Option<(u64, u64)>,
    /// Sorted unique drawn IDs, retaining capacity across unchanged frames.
    drawn: Vec<EntityRigId>,
}

impl StagedPack {
    pub(super) fn is_empty(&self) -> bool {
        self.geometries.is_empty()
    }
}

/// Either session pack namespace.
fn is_session_pack_rig(id: EntityRigId) -> bool {
    is_pack_rig_id(id) || is_pack_equipment_rig_id(id)
}

impl ActorRigFrameBuilder {
    /// Registers a geometry batch, reclaiming the oldest staged pages only as needed.
    /// Replaced pages are released first; an impossible batch leaves all residents intact.
    pub(super) fn insert_reclaiming_staged(
        &mut self,
        mut geometries: Vec<ActorRigGeometry>,
        revision: u64,
    ) -> Result<(), ActorRigGeometryError> {
        if self.staged.is_empty() {
            return self.catalog.append(geometries, revision);
        }
        for geometry in &mut geometries {
            geometry.revalidate()?;
        }
        // Match append's last-write-wins handling before planning the batch's storage.
        let added: BTreeMap<_, _> = geometries
            .into_iter()
            .map(|geometry| (geometry.id, geometry))
            .collect();
        let mut old: Vec<_> = self
            .catalog
            .geometries
            .keys()
            .filter(|id| self.staged.geometries.contains_key(id) && !added.contains_key(id))
            .map(|id| (self.staged.last_drawn.get(id).copied().unwrap_or(0), *id))
            .collect();
        old.sort_unstable();
        let evictable: Vec<_> = added
            .keys()
            .filter(|id| self.catalog.geometries.contains_key(id))
            .copied()
            .chain(old.into_iter().map(|(_, id)| id))
            .collect();
        let added: Vec<_> = added.into_values().collect();
        let plan = self.catalog.plan_admission(&added, &evictable, usize::MAX);
        if plan.admitted.len() != added.len() {
            return Err(ActorRigGeometryError::CatalogCapacity);
        }
        let evicted: BTreeSet<_> = evictable[..plan.evicted].iter().copied().collect();
        let staged_updates: Vec<_> = added
            .iter()
            .filter(|geometry| is_session_pack_rig(geometry.id))
            .cloned()
            .collect();
        self.catalog
            .append_evicting(added, |id| evicted.contains(id), revision)?;
        self.staged.geometries.extend(
            staged_updates
                .into_iter()
                .map(|geometry| (geometry.id, geometry)),
        );
        self.staged.last_drawn.retain(|id, _| !evicted.contains(id));
        self.staged.denied = None;
        Ok(())
    }

    /// Replaces both pack namespaces at once; `Err` only for geometry that fails validation,
    /// leaving the catalog unchanged.
    pub fn replace_session_pack_geometries(
        &mut self,
        entities: Vec<ActorRigGeometry>,
        equipment: Vec<ActorRigGeometry>,
    ) -> (
        Result<(), ActorRigGeometryError>,
        Result<(), ActorRigGeometryError>,
    ) {
        if entities.is_empty() && equipment.is_empty() && self.session_pack().is_empty() {
            return (Ok(()), Ok(()));
        }
        let mut pack = self.session_pack();
        replace_range(&mut pack, is_pack_rig_id, entities.iter().cloned());
        replace_range(
            &mut pack,
            is_pack_equipment_rig_id,
            equipment.iter().cloned(),
        );
        if self.publish_session_pack(pack).is_ok() {
            return (Ok(()), Ok(()));
        }
        let entities = self.replace_pack_geometries(entities);
        let equipment = self.replace_pack_equipment_geometries(equipment);
        (entities, equipment)
    }

    /// Replaces one pack namespace, keeping the other.
    pub(super) fn replace_range_geometries(
        &mut self,
        in_range: fn(EntityRigId) -> bool,
        geometries: Vec<ActorRigGeometry>,
    ) -> Result<(), ActorRigGeometryError> {
        let mut pack = self.session_pack();
        if geometries.is_empty() && !pack.keys().any(|id| in_range(*id)) {
            return Ok(());
        }
        replace_range(&mut pack, in_range, geometries);
        self.publish_session_pack(pack)
    }

    /// Sets the catalog's vertex budget and republishes the session pack under it.
    pub fn set_vertex_budget(&mut self, maximum_vertices: usize) {
        if maximum_vertices == self.catalog.maximum_vertices {
            return;
        }
        let previous = self.catalog.maximum_vertices;
        self.catalog.maximum_vertices = maximum_vertices;
        let pack = self.session_pack();
        if !pack.is_empty() && self.publish_session_pack(pack).is_err() {
            // Validated geometry only fails when the startup catalog alone exceeds the budget.
            self.catalog.maximum_vertices = previous;
        }
    }

    /// Every geometry of both pack namespaces, resident or staged.
    fn session_pack(&self) -> BTreeMap<EntityRigId, ActorRigGeometry> {
        if !self.staged.is_empty() {
            return self.staged.geometries.clone();
        }
        let mut pack = self.catalog.geometries.clone();
        pack.retain(|id, _| is_session_pack_rig(*id));
        pack
    }

    /// Makes `pack` the session pack: wholly resident when it fits the budget, otherwise
    /// staged for per-frame admission. On error the catalog is unchanged.
    fn publish_session_pack(
        &mut self,
        mut pack: BTreeMap<EntityRigId, ActorRigGeometry>,
    ) -> Result<(), ActorRigGeometryError> {
        // Keep refreshed preparation in both the resident and staged publication paths.
        for geometry in pack.values_mut() {
            geometry.revalidate()?;
        }
        let maximum = self.catalog.maximum_vertices;
        let mut base = self.catalog.geometries.clone();
        base.retain(|id, _| !is_session_pack_rig(*id));
        let mut all = base.clone();
        all.extend(pack.iter().map(|(id, geometry)| (*id, geometry.clone())));
        let staged = match GeometryCatalog::layout_with_limit(all, maximum) {
            Ok(catalog) => {
                self.catalog = catalog;
                BTreeMap::new()
            }
            // Layout validates every geometry before it checks capacity.
            Err(ActorRigGeometryError::CatalogCapacity) => {
                self.catalog = GeometryCatalog::layout_with_limit(base, maximum)?;
                pack
            }
            Err(error) => return Err(error),
        };
        self.staged = StagedPack {
            geometries: staged,
            ..StagedPack::default()
        };
        self.matrices = PoseMatrixCache::default();
        Ok(())
    }

    /// Makes staged rigs that `drawn` (the frame's surviving, visible submissions) needs
    /// resident: smallest first, adding up to [`STAGED_ADMISSION_VERTICES_PER_FRAME`] vertices
    /// per frame (always at least one rig), evicting the least recently drawn staged rigs the
    /// frame does not draw. Evicted pages are vacated in place, so retained GPU pages are not
    /// re-uploaded. A rig that cannot fit waits until the catalog or the drawn set changes.
    pub(super) fn admit_staged<'a>(
        &mut self,
        drawn: impl Iterator<Item = &'a ActorRigSubmission>,
        frame_generation: u64,
    ) {
        if self.staged.is_empty() {
            return;
        }
        let geometries = &self.staged.geometries;
        let drawn_rigs = &mut self.staged.drawn;
        drawn_rigs.clear();
        drawn_rigs.extend(drawn.filter_map(|submission| {
            let (id, geometry) = eligibility::geometry(geometries, submission).ok()?;
            (u32::try_from(submission.input.reset_generation).is_ok()
                && self.matrices.pose_is_valid(
                    &submission.input.previous_bones,
                    id,
                    &geometry.bone_pivots,
                )
                && self.matrices.pose_is_valid(
                    &submission.input.current_bones,
                    id,
                    &geometry.bone_pivots,
                ))
            .then_some(id)
        }));
        drawn_rigs.sort_unstable();
        drawn_rigs.dedup();
        let drawn = &self.staged.drawn;
        let mut missing = false;
        for id in drawn {
            if self.catalog.geometries.contains_key(id) {
                self.staged.last_drawn.insert(*id, frame_generation);
            } else {
                missing = true;
            }
        }
        if !missing {
            return;
        }
        let mut hasher = DefaultHasher::new();
        drawn.hash(&mut hasher);
        let key = (self.catalog.revision, hasher.finish());
        if self.staged.denied == Some(key) {
            return;
        }
        let mut candidates: Vec<_> = drawn
            .iter()
            .filter(|id| !self.catalog.geometries.contains_key(id))
            .map(|id| self.staged.geometries[id].clone())
            .collect();
        candidates.sort_by_key(|geometry| (geometry.vertices.len(), geometry.id));
        let mut evictable: Vec<_> = self
            .catalog
            .geometries
            .keys()
            .filter(|id| {
                self.staged.geometries.contains_key(id) && drawn.binary_search(id).is_err()
            })
            .map(|id| (self.staged.last_drawn.get(id).copied().unwrap_or(0), *id))
            .collect();
        evictable.sort_unstable();
        let evictable: Vec<_> = evictable.into_iter().map(|(_, id)| id).collect();
        let plan = self.catalog.plan_admission(
            &candidates,
            &evictable,
            STAGED_ADMISSION_VERTICES_PER_FRAME,
        );
        let mut denied = plan.denied;
        if !plan.admitted.is_empty() {
            let evict: BTreeSet<_> = evictable[..plan.evicted].iter().copied().collect();
            let admitted: Vec<_> = plan
                .admitted
                .iter()
                .map(|&index| candidates[index].clone())
                .collect();
            let revision = self.appended_revision(&admitted);
            if self
                .catalog
                .append_evicting(admitted, |id| evict.contains(id), revision)
                .is_ok()
            {
                self.staged.last_drawn.retain(|id, _| !evict.contains(id));
                self.staged.last_drawn.extend(
                    plan.admitted
                        .iter()
                        .map(|&index| (candidates[index].id, frame_generation)),
                );
            } else {
                // Fragmentation cannot refuse a planned fit, so this is defensive only.
                denied = true;
            }
        }
        // A deferred rig retries next frame; only a pure denial waits for a change.
        if denied && !plan.deferred {
            self.staged.denied = Some((self.catalog.revision, key.1));
        }
    }
}

/// Upload bound for staged geometry admitted in one frame (3.5 MiB of vertices).
const STAGED_ADMISSION_VERTICES_PER_FRAME: usize = 65_536;

/// Replaces one namespace, ignoring incoming geometry outside that namespace.
pub(super) fn replace_range(
    catalog: &mut BTreeMap<EntityRigId, ActorRigGeometry>,
    in_range: fn(EntityRigId) -> bool,
    geometries: impl IntoIterator<Item = ActorRigGeometry>,
) {
    catalog.retain(|id, _| !in_range(*id));
    catalog.extend(
        geometries
            .into_iter()
            .filter(|geometry| in_range(geometry.id))
            .map(|geometry| (geometry.id, geometry)),
    );
}

#[cfg(test)]
#[path = "pack/tests.rs"]
mod tests;
