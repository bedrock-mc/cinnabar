//! Publishes a session's entity and equipment ranges under one catalog layout, and admits
//! staged ranges as frames draw them.

use std::collections::BTreeSet;

use super::*;
use crate::ActorRigFrameBuilder;

impl ActorRigFrameBuilder {
    /// Combines valid updates while retaining sequential rejection and partial acceptance.
    pub fn replace_session_pack_geometries(
        &mut self,
        entities: Vec<ActorRigGeometry>,
        equipment: Vec<ActorRigGeometry>,
    ) -> (
        Result<(), ActorRigGeometryError>,
        Result<(), ActorRigGeometryError>,
    ) {
        if entities.is_empty()
            && equipment.is_empty()
            && !self
                .catalog
                .geometries
                .keys()
                .any(|id| is_pack_rig_id(*id) || is_pack_equipment_rig_id(*id))
            && self.staged.is_empty()
        {
            return (Ok(()), Ok(()));
        }
        let mut combined = self.catalog.geometries.clone();
        replace_range(&mut combined, is_pack_rig_id, entities.iter().cloned());
        // The first update must fit with the previous equipment still present.
        if self.catalog.can_admit(&combined) {
            replace_range(
                &mut combined,
                is_pack_equipment_rig_id,
                equipment.iter().cloned(),
            );
            if let Ok(catalog) =
                GeometryCatalog::layout_with_limit(combined, self.catalog.maximum_vertices)
            {
                self.catalog = catalog;
                self.staged.clear();
                return (Ok(()), Ok(()));
            }
        }
        let entities = self.replace_pack_geometries(entities);
        let equipment = self.replace_pack_equipment_geometries(equipment);
        (entities, equipment)
    }

    /// Makes staged geometry these submissions draw resident, at most
    /// [`STAGED_ADMISSION_VERTICES_PER_FRAME`] vertices per frame (always at least one rig).
    /// A full catalog evicts staged geometry they do not draw, in place, so retained pages are
    /// not re-uploaded; a rig that still does not fit stays a missing-geometry reject.
    pub(super) fn admit_staged(&mut self, submissions: &[ActorRigSubmission]) {
        if self.staged.is_empty() {
            return;
        }
        let drawn: BTreeSet<EntityRigId> = submissions
            .iter()
            .filter(|submission| {
                matches!(
                    submission.route,
                    ActorRigRoute::Compiled | ActorRigRoute::StaticFallback
                )
            })
            .map(|submission| submission.input.rig)
            .filter(|id| self.staged.contains_key(id))
            .collect();
        let mut budget = STAGED_ADMISSION_VERTICES_PER_FRAME;
        let mut batch = Vec::new();
        for id in &drawn {
            if self.catalog.geometries.contains_key(id) {
                continue;
            }
            let geometry = &self.staged[id];
            if !batch.is_empty() && geometry.vertices.len() > budget {
                break;
            }
            budget = budget.saturating_sub(geometry.vertices.len());
            batch.push(geometry.clone());
        }
        if batch.is_empty() {
            return;
        }
        let revision = self.appended_revision(&batch);
        if self.catalog.append(batch.clone(), revision).is_ok() {
            return;
        }
        let staged = &self.staged;
        let undrawn = |id: &EntityRigId| staged.contains_key(id) && !drawn.contains(id);
        for geometry in batch {
            let revision = self.appended_revision(std::slice::from_ref(&geometry));
            let _ = self
                .catalog
                .append_evicting(vec![geometry], undrawn, revision);
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
