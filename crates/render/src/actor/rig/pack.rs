//! Publishes a session's entity and equipment ranges under one catalog layout.
use super::*;

impl ActorRigFrameBuilder {
    /// Combines valid updates while retaining sequential rejection and partial acceptance.
    pub(in crate::actor) fn replace_session_pack_geometries(
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
        {
            return (Ok(()), Ok(()));
        }
        let mut combined = self.catalog.geometries.clone();
        replace_range(&mut combined, is_pack_rig_id, entities.iter().cloned());
        // The first update must fit with the previous equipment still present.
        if combined
            .values()
            .map(|geometry| geometry.vertices.len())
            .sum::<usize>()
            <= MAX_ACTOR_RIG_VERTICES
        {
            replace_range(
                &mut combined,
                is_pack_equipment_rig_id,
                equipment.iter().cloned(),
            );
            if let Ok(catalog) = GeometryCatalog::layout(combined) {
                self.catalog = catalog;
                return (Ok(()), Ok(()));
            }
        }
        let entities = self.replace_pack_geometries(entities);
        let equipment = self.replace_pack_equipment_geometries(equipment);
        (entities, equipment)
    }
}

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
