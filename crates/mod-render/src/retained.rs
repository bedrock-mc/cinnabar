//! Owner-scoped immutable geometry and transactional drawing instances.

use mod_api::drawing::{DrawingHandle as Handle, DrawingPermissions};
use std::{collections::BTreeMap, sync::Arc};

mod entities;
mod materials;
pub mod mesh;
pub use entities::EntityKey;
#[cfg(test)]
mod tests;
pub mod transfer;
mod validation;

/// Linear vertex inputs for shared unlit drawing materials.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Vertex {
    pub position: [f32; 3],
    pub color: [f32; 4],
    pub uv: [f32; 2],
    pub data: [f32; 4],
}

/// Immutable topology; polyline distances are computed by the host on admission.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum GeometrySpec {
    Triangles {
        vertices: Vec<Vertex>,
        indices: Vec<u32>,
    },
    Polyline {
        vertices: Vec<Vertex>,
        closed: bool,
    },
}

/// Validated geometry and its cached facts stay immutable across instances and snapshots.
#[derive(Clone, Debug, PartialEq)]
pub struct Geometry {
    vertices: Arc<[Vertex]>,
    bounds: [[f32; 3]; 2],
    indices: Arc<[u32]>,
    distances: Arc<[f32]>,
    closed: bool,
    polyline: bool,
    radius: f32,
    bytes: usize,
    expanded_vertices: usize,
}

impl Geometry {
    /// Returns the local position bounds computed during admission.
    pub fn bounds(&self) -> [[f32; 3]; 2] {
        self.bounds
    }

    /// Counts the immutable native storage needed by this admitted mesh.
    pub fn gpu_bytes(&self) -> u64 {
        (self.expanded_vertices * std::mem::size_of::<mesh::GpuVertex>()) as u64
    }

    /// Reports admitted triangles without trusting a guest-supplied cost.
    pub fn triangles(&self) -> usize {
        self.indices.len() / 3
    }

    /// Reports whether line expansion and width handling apply to this geometry.
    pub fn is_polyline(&self) -> bool {
        self.polyline
    }
}

/// An actor reference includes both the drawing owner and the actor's spawn generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EntityAnchor {
    pub entity: Handle,
    pub body_rotation: bool,
}

/// Placement never grants observations of the anchored world or block.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Anchor {
    World { dimension: i32, origin: [f64; 3] },
    Block { dimension: i32, position: [i32; 3] },
    Camera,
    Entity(EntityAnchor),
}

/// Local translation, normalized XYZW rotation and scale, applied after the anchor.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Transform {
    pub translation: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}
impl Default for Transform {
    /// Identity placement preserves geometry coordinates.
    fn default() -> Self {
        Self {
            translation: [0.; 3],
            rotation: [0., 0., 0., 1.],
            scale: [1.; 3],
        }
    }
}

#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
)]
pub enum Depth {
    #[default]
    Test,
    Always,
}
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
)]
pub enum Blend {
    #[default]
    Premultiplied,
    Additive,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum WidthMode {
    #[default]
    World,
    Pixels,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Join {
    #[default]
    Bevel,
    Miter,
    Round,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Cap {
    #[default]
    Butt,
    Round,
}

/// Instances select the shared unlit material or an owner-scoped procedural fragment.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Instance {
    pub geometry: Handle,
    pub material: Option<Handle>,
    pub params: [f32; mod_api::materials::PARAMS],
    pub anchor: Anchor,
    pub transform: Transform,
    pub tint: [f32; 4],
    pub depth: Depth,
    pub blend: Blend,
    pub width: f32,
    pub width_mode: WidthMode,
    pub join: Join,
    pub cap: Cap,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorCode {
    Denied,
    Invalid,
    BudgetExceeded,
    StaleHandle,
    Unsupported,
    InUse,
}
/// Stable error categories with a bounded diagnostic identifying the rejected resource.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrawError {
    pub code: ErrorCode,
    pub detail: &'static str,
}

/// A successful callback's immutable snapshot tracks geometry, materials and instances separately.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Resources {
    pub connection: u64,
    pub dimension: i32,
    pub entities: BTreeMap<Handle, EntityKey>,
    pub geometry: BTreeMap<Handle, Arc<Geometry>>,
    pub instances: BTreeMap<Handle, Instance>,
    pub materials: BTreeMap<Handle, Arc<crate::material::Material>>,
    groups: BTreeMap<String, Vec<Handle>>,
    pub geometry_revision: u64,
    pub material_revision: u64,
    /// Changes when placements or their entity references change.
    pub instance_revision: u64,
}

/// A store owns a single guest incarnation. Tokens are never reused after rollback.
pub struct Store {
    owner: u64,
    generation: u64,
    next_slot: u64,
    committed: Arc<Resources>,
    staged: Option<Resources>,
    mutation_bytes: usize,
    retired: Vec<Arc<Geometry>>,
    retired_materials: Vec<Arc<crate::material::Material>>,
    material_compiles: u32,
    entity_candidates: Vec<EntityKey>,
}

impl Default for Store {
    /// Assigns a process-unique owner even to a replacement guest.
    fn default() -> Self {
        Self {
            owner: crate::identity::next_owner(),
            generation: 1,
            next_slot: 0,
            committed: Arc::default(),
            staged: None,
            mutation_bytes: 0,
            retired: Vec::new(),
            retired_materials: Vec::new(),
            material_compiles: 0,
            entity_candidates: Vec::new(),
        }
    }
}

impl Store {
    /// Binds snapshots to the trusted connection before the guest initializes resources.
    pub fn bind_session(&mut self, connection: u64, dimension: i32) {
        self.revoke();
        self.committed = Arc::new(Resources {
            connection,
            dimension,
            ..Default::default()
        });
    }

    /// Attaches initial callback resources to their first native session without recycling tokens.
    /// A previously bound session must use `bind_session`, which revokes its resources.
    pub fn attach_session(&mut self, connection: u64, dimension: i32) -> bool {
        if self.committed.connection != 0 {
            return false;
        }
        let mut resources = (*self.committed).clone();
        resources.connection = connection;
        resources.dimension = dimension;
        self.committed = Arc::new(resources);
        true
    }

    /// Returns the last successful callback without copying its resources.
    pub fn snapshot(&self) -> &Arc<Resources> {
        &self.committed
    }

    /// Begins a callback; immediate groups expire only if this callback commits.
    pub fn begin(&mut self) {
        self.staged = None;
        self.mutation_bytes = 0;
        self.material_compiles = 0;
    }

    /// Publishes all staged changes together and preserves identity on unchanged callbacks.
    pub fn commit(&mut self) {
        let staged = match self.staged.take() {
            Some(staged) => Some(staged),
            None if self.committed.groups.is_empty() => None,
            None => Some(self.base()),
        };
        if let Some(staged) = staged
            && (staged.geometry_revision != self.committed.geometry_revision
                || staged.material_revision != self.committed.material_revision
                || staged.instance_revision != self.committed.instance_revision)
        {
            self.committed = Arc::new(staged);
        }
        self.retired_materials
            .retain(|material| Arc::strong_count(material) > 1);
        self.retired
            .retain(|geometry| Arc::strong_count(geometry) > 1);
    }

    /// Discards mutations without recycling any tokens returned by the failed callback.
    pub fn rollback(&mut self) {
        self.staged = None;
        self.mutation_bytes = 0;
    }

    /// Invalidates all resource handles and clears output on trap, reload or session changes.
    pub fn revoke(&mut self) {
        self.generation = self
            .generation
            .checked_add(1)
            .expect("drawing generation exhausted");
        self.committed = Arc::default();
        self.staged = None;
        self.retired.clear();
        self.retired_materials.clear();
        self.material_compiles = 0;
        self.entity_candidates.clear();
    }

    /// Charges copied asset bytes against this callback's shared mutation allowance.
    pub fn charge_bytes(&mut self, bytes: usize) -> Result<(), DrawError> {
        let total = self
            .mutation_bytes
            .checked_add(bytes)
            .ok_or(error(ErrorCode::BudgetExceeded, "mutation bytes"))?;
        if total > mod_api::MAX_DRAW_MUTATION_BYTES {
            return Err(error(ErrorCode::BudgetExceeded, "mutation bytes"));
        }
        self.mutation_bytes = total;
        Ok(())
    }

    /// Validates and retains an immutable mesh, charging live and retiring resources.
    pub fn create_geometry(&mut self, spec: GeometrySpec) -> Result<Handle, DrawError> {
        let geometry = Arc::new(validation::geometry(spec)?);
        let bytes = geometry.bytes;
        let handle = self.token()?;
        self.edit(bytes, |resources| {
            resources.geometry.insert(handle, geometry);
            resources.geometry_revision += 1;
            Ok(())
        })?;
        Ok(handle)
    }

    /// Creates one independently movable instance after validating references and authority.
    pub fn create(
        &mut self,
        instance: Instance,
        permissions: DrawingPermissions,
    ) -> Result<Handle, DrawError> {
        let handle = self.token()?;
        self.edit(std::mem::size_of::<Instance>(), |resources| {
            validation::instance(&instance, resources, permissions)?;
            resources.instances.insert(handle, instance);
            resources.instance_revision += 1;
            Ok(())
        })?;
        Ok(handle)
    }

    /// Replaces only instance data; the immutable geometry allocation stays shared.
    pub fn update(
        &mut self,
        handle: Handle,
        instance: Instance,
        permissions: DrawingPermissions,
    ) -> Result<(), DrawError> {
        self.check(handle)?;
        // An unchanged resubmission must not clone the resource maps every frame.
        let current = self.staged.as_ref().unwrap_or(&self.committed);
        let expiring = self.staged.is_none()
            && current
                .groups
                .values()
                .any(|handles| handles.contains(&handle));
        if !expiring && current.instances.get(&handle) == Some(&instance) {
            return validation::instance(&instance, current, permissions);
        }
        self.edit(std::mem::size_of::<Instance>(), |resources| {
            if !resources.instances.contains_key(&handle) {
                return Err(error(ErrorCode::StaleHandle, "drawing"));
            }
            validation::instance(&instance, resources, permissions)?;
            if resources.instances[&handle] != instance {
                resources.instances.insert(handle, instance);
                resources.instance_revision += 1;
            }
            Ok(())
        })
    }

    /// Replaces a named callback-only group using the same admission and instance storage.
    pub fn replace_group(
        &mut self,
        name: String,
        instances: Vec<Instance>,
        permissions: DrawingPermissions,
    ) -> Result<(), DrawError> {
        if name.is_empty() || name.len() > mod_api::MAX_DRAW_GROUP_NAME_BYTES {
            return Err(error(ErrorCode::Invalid, "group name"));
        }
        if instances.len() > mod_api::MAX_DRAW_INSTANCES {
            return Err(error(ErrorCode::BudgetExceeded, "instances"));
        }
        let handles = (0..instances.len())
            .map(|_| self.token())
            .collect::<Result<Vec<_>, _>>()?;
        self.edit(
            instances.len() * std::mem::size_of::<Instance>() + name.len(),
            |resources| {
                if let Some(previous) = resources.groups.remove(&name) {
                    for handle in previous {
                        resources.instances.remove(&handle);
                    }
                }
                for (handle, instance) in handles.iter().zip(instances) {
                    validation::instance(&instance, resources, permissions)?;
                    resources.instances.insert(*handle, instance);
                }
                if !handles.is_empty() {
                    resources.groups.insert(name, handles);
                }
                resources.instance_revision += 1;
                Ok(())
            },
        )
    }

    /// Releases an instance or unused geometry; referenced geometry returns an explicit error.
    pub fn remove(&mut self, handle: Handle) -> Result<(), DrawError> {
        self.check(handle)?;
        let mut retired = None;
        let mut retired_material = None;
        self.edit(0, |resources| {
            if resources.entities.contains_key(&handle) {
                if resources.instances.values().any(|instance| matches!(instance.anchor, Anchor::Entity(anchor) if anchor.entity == handle)) { return Err(error(ErrorCode::InUse, "entity")); }
                resources.entities.remove(&handle);
                resources.instance_revision += 1;
                return Ok(());
            }

            if resources.instances.remove(&handle).is_some() {
                resources.instance_revision += 1;
                return Ok(());
            }
            if resources
                .instances
                .values()
                .any(|instance| instance.geometry == handle)
            {
                return Err(error(ErrorCode::InUse, "geometry"));
            }
            if resources
                .instances
                .values()
                .any(|instance| instance.material == Some(handle))
            {
                return Err(error(ErrorCode::InUse, "material"));
            }
            retired_material = resources.materials.remove(&handle);
            if retired_material.is_some() {
                resources.material_revision += 1;
                return Ok(());
            }
            retired = resources.geometry.remove(&handle);
            if retired.is_none() {
                return Err(error(ErrorCode::StaleHandle, "resource"));
            }
            resources.geometry_revision += 1;
            Ok(())
        })?;
        if let Some(material) = retired_material
            && !self
                .retired_materials
                .iter()
                .any(|old| Arc::ptr_eq(old, &material))
        {
            self.retired_materials.push(material);
        }
        if let Some(geometry) = retired
            && !self.retired.iter().any(|old| Arc::ptr_eq(old, &geometry))
        {
            self.retired.push(geometry);
        }
        Ok(())
    }

    /// Derives callback state and drops last callback's immediate instances atomically.
    fn base(&self) -> Resources {
        let mut resources = self.staged.as_ref().unwrap_or(&self.committed).clone();
        if self.staged.is_none() && !resources.groups.is_empty() {
            for handles in resources.groups.values() {
                for handle in handles {
                    resources.instances.remove(handle);
                }
            }
            resources.groups.clear();
            resources.instance_revision += 1;
        }
        resources
    }

    /// Validates a candidate before installing any part of one import's mutation.
    fn edit(
        &mut self,
        bytes: usize,
        change: impl FnOnce(&mut Resources) -> Result<(), DrawError>,
    ) -> Result<(), DrawError> {
        let total = self
            .mutation_bytes
            .checked_add(bytes)
            .ok_or(error(ErrorCode::BudgetExceeded, "mutation bytes"))?;
        if total > mod_api::MAX_DRAW_MUTATION_BYTES {
            return Err(error(ErrorCode::BudgetExceeded, "mutation bytes"));
        }
        let mut candidate = self.base();
        change(&mut candidate)?;
        validation::budget(&candidate, &self.retired)?;
        self.material_budget(&candidate)?;
        self.mutation_bytes = total;
        self.staged = Some(candidate);
        Ok(())
    }

    /// Allocates a token even if subsequent admission fails, so rollback cannot recycle it.
    fn token(&mut self) -> Result<Handle, DrawError> {
        self.next_slot = self
            .next_slot
            .checked_add(1)
            .ok_or(error(ErrorCode::BudgetExceeded, "handle slots"))?;
        Ok(Handle {
            owner: self.owner,
            generation: self.generation,
            slot: self.next_slot,
        })
    }

    /// Rejects cross-owner and previous-incarnation tokens before reading resource tables.
    fn check(&self, handle: Handle) -> Result<(), DrawError> {
        if handle.owner != self.owner || handle.generation != self.generation {
            Err(error(ErrorCode::StaleHandle, "owner or generation"))
        } else {
            Ok(())
        }
    }
}

/// Constructs a bounded typed failure without echoing guest data into diagnostics.
fn error(code: ErrorCode, detail: &'static str) -> DrawError {
    DrawError { code, detail }
}
