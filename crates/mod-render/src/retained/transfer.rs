//! Portable drawing snapshots. Cached bounds and compiled shaders never cross the boundary.

use super::*;
use serde::{Deserialize, Serialize};

/// Immutable guest inputs, rechecked by the receiving host before rendering.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub geometry: Vec<(Handle, GeometrySpec)>,
    pub materials: Vec<(Handle, String)>,
    pub instances: Vec<(Handle, Instance)>,
    pub entities: Vec<(Handle, EntityKey)>,
}

impl Geometry {
    /// Recovers only original mesh inputs, excluding host-computed bounds and costs.
    fn spec(&self) -> GeometrySpec {
        if self.polyline {
            GeometrySpec::Polyline {
                vertices: self.vertices.to_vec(),
                closed: self.closed,
            }
        } else {
            GeometrySpec::Triangles {
                vertices: self.vertices.to_vec(),
                indices: self.indices.to_vec(),
            }
        }
    }
}

impl Snapshot {
    /// Reports whether this snapshot retains any geometry, material, instance or actor token.
    pub fn is_empty(&self) -> bool {
        let Self {
            geometry,
            materials,
            instances,
            entities,
        } = self;
        geometry.is_empty() && materials.is_empty() && instances.is_empty() && entities.is_empty()
    }

    /// Serializes committed resources without their renderer caches or native identifiers.
    pub fn from_resources(resources: &Resources) -> Self {
        Self {
            entities: resources
                .entities
                .iter()
                .map(|(&id, &key)| (id, key))
                .collect(),
            geometry: resources
                .geometry
                .iter()
                .map(|(&id, value)| (id, value.spec()))
                .collect(),
            materials: resources
                .materials
                .iter()
                .map(|(&id, value)| (id, value.source.to_string()))
                .collect(),
            instances: resources
                .instances
                .iter()
                .map(|(&id, value)| (id, value.clone()))
                .collect(),
        }
    }

    /// Revalidates topology, materials, placements and aggregate costs atomically. The caller
    /// supplies local tokens, so two helper processes cannot collide in the renderer.
    pub fn admit(
        &self,
        previous: Option<&Resources>,
        permissions: DrawingPermissions,
        connection: u64,
        dimension: i32,
        entities: &[EntityKey],
        mut token: impl FnMut(Handle) -> Handle,
    ) -> Result<Resources, DrawError> {
        if !permissions.world_draw && !self.is_empty() {
            return Err(error(ErrorCode::Denied, "world drawing"));
        }
        if self.geometry.len() > mod_api::MAX_DRAW_GEOMETRIES
            || self.instances.len() > mod_api::MAX_DRAW_INSTANCES
            || self.materials.len() > mod_api::MAX_DRAW_MATERIALS
        {
            return Err(error(ErrorCode::BudgetExceeded, "drawing resources"));
        }
        let mut resources = Resources {
            connection,
            dimension,
            ..Default::default()
        };
        let mut ids = std::collections::BTreeSet::new();
        if self.entities.len() > mod_api::MAX_DRAW_ENTITIES {
            return Err(error(ErrorCode::BudgetExceeded, "entity references"));
        }
        for (id, key) in &self.entities {
            if !permissions.entity_observe
                || key.session != connection
                || key.dimension != dimension
                || !entities.contains(key)
            {
                return Err(error(ErrorCode::Denied, "entity reference"));
            }
            let id = token(*id);
            if !ids.insert(id) {
                return Err(error(ErrorCode::Invalid, "duplicate resource"));
            }
            resources.entities.insert(id, *key);
        }
        for (id, spec) in &self.geometry {
            let id = token(*id);
            if !ids.insert(id) {
                return Err(error(ErrorCode::Invalid, "duplicate resource"));
            }
            let geometry = match previous
                .and_then(|old| old.geometry.get(&id))
                .filter(|old| old.spec() == *spec)
            {
                Some(old) => Arc::clone(old),
                None => Arc::new(validation::geometry(spec.clone())?),
            };
            resources.geometry.insert(id, geometry);
        }
        for (id, source) in &self.materials {
            if !permissions.render_material {
                return Err(error(ErrorCode::Denied, "material authority"));
            }
            let id = token(*id);
            if !ids.insert(id) {
                return Err(error(ErrorCode::Invalid, "duplicate resource"));
            }
            let material = match previous
                .and_then(|old| old.materials.get(&id))
                .filter(|old| old.source.as_ref() == source)
            {
                Some(old) => Arc::clone(old),
                None => Arc::new(crate::material::Material {
                    source: source.as_str().into(),
                    shader: crate::material::compose(source)
                        .map_err(|_| error(ErrorCode::Invalid, "material shader"))?
                        .into(),
                }),
            };
            resources.materials.insert(id, material);
        }
        for (id, instance) in &self.instances {
            let id = token(*id);
            if !ids.insert(id) {
                return Err(error(ErrorCode::Invalid, "duplicate resource"));
            }
            let mut instance = instance.clone();
            instance.geometry = token(instance.geometry);
            instance.material = instance.material.map(&mut token);
            if let Anchor::Entity(anchor) = &mut instance.anchor {
                anchor.entity = token(anchor.entity);
            }
            validation::instance(&instance, &resources, permissions)?;
            resources.instances.insert(id, instance);
        }
        validation::budget(&resources, &[])?;
        resources.geometry_revision = previous.map_or(1, |old| {
            old.geometry_revision + u64::from(old.geometry != resources.geometry)
        });
        resources.material_revision = previous.map_or(1, |old| {
            old.material_revision + u64::from(old.materials != resources.materials)
        });
        resources.instance_revision = previous.map_or(1, |old| {
            old.instance_revision
                + u64::from(
                    old.instances != resources.instances || old.entities != resources.entities,
                )
        });
        Ok(resources)
    }

    /// Checks resident and retiring topology against the same budgets as local drawing.
    pub fn check_budget(resources: &Resources, retired: &[Arc<Geometry>]) -> Result<(), DrawError> {
        validation::budget(resources, retired)
    }

    /// Counts expanded GPU geometry, including the per-instance record allocation.
    pub fn gpu_bytes(resources: &Resources) -> u64 {
        let vertices: usize = resources
            .geometry
            .values()
            .map(|geometry| geometry.expanded_vertices)
            .sum();
        (vertices * std::mem::size_of::<mesh::GpuVertex>()
            + if resources.instances.is_empty() {
                0
            } else {
                (resources.instances.len() * mod_api::drawing::INSTANCE_GPU_BYTES)
                    .next_power_of_two()
            }) as u64
    }
}
