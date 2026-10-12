//! Runtime-ID collision registries for both Bedrock palette identity modes.
//!
//! Compiled shape and material facts are indexed without deriving state geometry.

use std::path::{Path, PathBuf};
use std::{collections::BTreeMap, ops::Range, sync::Arc};

use assets::RegistryRecord;
use assets::block_shapes::COLLISION_COORDINATE_SCALE;
use sim::{
    Aabb, CollisionIdSpace, CollisionRegistry, CollisionRegistryIdentity, RegistryError, Vec3,
};
use thiserror::Error;

mod multipart;
mod selection;
mod session_palette;
mod tags;

/// Registry records whose names the carrier deliberately withholds.
const RESERVED_RECORD_NAME: &str = "cinnabar:reserved";

/// Interaction facts shared by both runtime identity spaces.
#[derive(Debug, Clone)]
struct InteractionBlock {
    identifier: Arc<str>,
    full_cube: bool,
    build_intention: bool,
    shape_flags: assets::block_shapes::ShapeFlags,
    tags: Arc<[Arc<str>]>,
    connections: Option<(
        assets::ModelFamily,
        assets::ContributorRole,
        bool,
        Option<u32>,
    )>,
    /// Offset to the other block of a two-block structure such as a door or bed.
    second_part: Option<[i32; 3]>,
}

type InteractionBlocks = BTreeMap<u32, InteractionBlock>;

/// Runtime-ID collision registries for both Bedrock palette identity modes.
///
/// The two maps are intentionally distinct: a 32-bit network hash may have
/// the same numeric value as an unrelated sequential ID.
#[derive(Debug)]
pub struct PhysicsCollisionRegistries {
    sequential: CollisionRegistry,
    hashed: CollisionRegistry,
    available_record_count: usize,
    sequential_count: usize,
    hashed_count: usize,
    preg_sha256: [u8; 32],
    breg_sha256: [u8; 32],
    interaction_blocks: InteractionBlocks,
    hashed_interaction_blocks: InteractionBlocks,
    /// Named vanilla blocks as `(sort key, name, first sequential id)`, in id order.
    vanilla_runs: Vec<(u64, Arc<str>, u32)>,
    /// False when the carrier's ids are not in vanilla sort order, so customs cannot interleave.
    interleave_supported: bool,
    canonical_states: BTreeMap<u32, Arc<str>>,
    hashed_canonical_states: BTreeMap<u32, Arc<str>>,
    state_ids: BTreeMap<Arc<str>, BTreeMap<String, (u32, u32)>>,
    max_vanilla_sort_key: u64,
    /// Hashes this session added to `hashed`, dropped when the next session begins.
    session_hashes: Vec<u32>,
}

#[derive(Debug, Error)]
pub enum PhysicsCollisionRegistryError {
    #[error(transparent)]
    Asset(#[from] assets::AssetError),
    #[error(transparent)]
    Registry(#[from] RegistryError),
    /// A byte-valid physics registry whose stamped header protocol disagrees
    /// with the active content authority. Distinct from every generic decode
    /// failure so a partially flipped carrier set is attributable at a
    /// glance: the message names both protocols and both artifact paths.
    #[error(
        "cross-carrier registry protocol mismatch: physics registry {physics_registry_path} stamps wire protocol {actual_protocol} but this startup binds active content protocol {expected_protocol} alongside world carrier {world_carrier_path}; rebuild the flipped side so both carriers carry the same registry protocol"
    )]
    ProtocolMismatch {
        expected_protocol: u32,
        actual_protocol: u32,
        physics_registry_path: PathBuf,
        world_carrier_path: PathBuf,
    },
}

impl PhysicsCollisionRegistries {
    /// Binds the pinned BREG and a sha-verified installed PREG under one
    /// explicit content-registry protocol.
    ///
    /// This is the production startup seam for the cross-carrier coherence
    /// gate: before any decode it compares the PREG's stamped header protocol
    /// against `expected_protocol` and fails closed with
    /// [`PhysicsCollisionRegistryError::ProtocolMismatch`], naming both
    /// protocols and both artifact paths (the installed physics registry and
    /// the loaded world carrier). Without this comparison, a future partial
    /// flip (world 2193 + physics 1001 or reverse) would recreate the live
    /// block-identity aliasing mechanism with zero decode errors. Malformed
    /// headers fall through to the full decoder so structural errors keep
    /// their precise existing messages.
    pub fn bind_coherent_assets(
        breg_bytes: &[u8],
        preg_bytes: &[u8],
        preg_path: &Path,
        world_carrier_path: &Path,
        expected_protocol: u32,
    ) -> Result<Self, PhysicsCollisionRegistryError> {
        match assets::physics_registry_header_protocol(preg_bytes) {
            Ok(actual_protocol) if actual_protocol != expected_protocol => {
                return Err(PhysicsCollisionRegistryError::ProtocolMismatch {
                    expected_protocol,
                    actual_protocol,
                    physics_registry_path: preg_path.to_path_buf(),
                    world_carrier_path: world_carrier_path.to_path_buf(),
                });
            }
            _ => {}
        }
        let records = assets::read_registry_for_protocol(breg_bytes, expected_protocol)?;
        Self::from_assets(breg_bytes, &records, preg_bytes, expected_protocol)
    }

    pub fn from_assets(
        breg_bytes: &[u8],
        records: &[RegistryRecord],
        preg_bytes: &[u8],
        expected_protocol: u32,
    ) -> Result<Self, PhysicsCollisionRegistryError> {
        let physics = assets::read_physics_registry_for_protocol(
            preg_bytes,
            breg_bytes,
            records,
            expected_protocol,
        )?;
        // The identity carries the decoder's exact stamped wire protocol so
        // the registries never claim a protocol the carrier did not declare;
        // the explicit `expected_protocol` argument keeps every construction
        // site on the shared active-content authority instead of a hidden
        // legacy default.
        let sequential_identity = CollisionRegistryIdentity {
            protocol: physics.protocol(),
            id_space: CollisionIdSpace::Sequential,
            preg_sha256: physics.sha256(),
        };
        let hashed_identity = CollisionRegistryIdentity {
            id_space: CollisionIdSpace::Hashed,
            ..sequential_identity
        };
        let mut sequential = CollisionRegistry::with_identity(sequential_identity);
        let mut hashed = CollisionRegistry::with_identity(hashed_identity);
        let mut interaction_blocks = BTreeMap::new();
        let mut hashed_interaction_blocks = BTreeMap::new();
        let mut canonical_states = BTreeMap::new();
        let mut hashed_canonical_states = BTreeMap::new();
        let mut state_ids: BTreeMap<Arc<str>, BTreeMap<String, (u32, u32)>> = BTreeMap::new();
        let mut max_vanilla_sort_key = 0;
        let mut vanilla_runs: Vec<(u64, Arc<str>, u32)> = Vec::new();
        for record in records {
            let fact = physics
                .by_sequential_id(record.sequential_id)
                .expect("strict PREG decoder covers every supplied BREG record");
            let shapes =
                fact.shapes
                    .as_ref()
                    .ok_or_else(|| assets::AssetError::InvalidPhysicsRegistry {
                        detail: "missing compiled block shapes; run make assets".into(),
                    })?;
            let boxes = fact
                .boxes
                .iter()
                .copied()
                .map(collision_box_to_aabb)
                .collect::<Vec<_>>();
            let full_cube = shapes
                .flags
                .contains(assets::block_shapes::ShapeFlags::FULL_CUBE);
            let states = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(
                &record.canonical_state,
            )
            .ok();
            let binding = InteractionBlock {
                identifier: Arc::from(record.name.as_ref()),
                full_cube,
                build_intention: shapes
                    .flags
                    .contains(assets::block_shapes::ShapeFlags::BUILD_INTENTION),
                shape_flags: shapes.flags,
                tags: shapes
                    .tags
                    .iter()
                    .map(|tag| Arc::from(tag.as_ref()))
                    .collect(),
                connections: Some((
                    record.model_family,
                    record.contributor_role,
                    record
                        .flags
                        .contains(assets::BlockFlags::OCCLUDES_FULL_FACE),
                    record.model_state.get(assets::ModelStateField::Orientation),
                )),
                second_part: states
                    .as_ref()
                    .and_then(|states| multipart::second_part_offset(&record.name, states)),
            };
            interaction_blocks.insert(record.sequential_id, binding.clone());
            hashed_interaction_blocks.insert(record.network_hash, binding);
            let state: Arc<str> = Arc::from(record.canonical_state.as_ref());
            canonical_states.insert(record.sequential_id, Arc::clone(&state));
            hashed_canonical_states.insert(record.network_hash, state);
            if let Some(states) = states {
                state_ids
                    .entry(Arc::from(record.name.as_ref()))
                    .or_default()
                    .insert(
                        serde_json::to_string(&states).expect("block state JSON is serializable"),
                        (record.sequential_id, record.network_hash),
                    );
            }
            let register = |registry: &mut CollisionRegistry, runtime_id, boxes: Vec<Aabb>| {
                registry.register_primitives(
                    runtime_id,
                    boxes,
                    f64::from(fact.friction_q1e8) * COLLISION_COORDINATE_SCALE,
                    f64::from(fact.horizontal_speed_q1e8) * COLLISION_COORDINATE_SCALE,
                    f64::from(fact.vertical_speed_q1e8) * COLLISION_COORDINATE_SCALE,
                    f64::from(fact.fluid_height_q1e8) * COLLISION_COORDINATE_SCALE,
                    fact.flags.bits(),
                    fact.surface_response as u8,
                )
            };
            register(&mut sequential, record.sequential_id, boxes.clone())?;
            register(&mut hashed, record.network_hash, boxes)?;
            for (registry, id) in [
                (&mut sequential, record.sequential_id),
                (&mut hashed, record.network_hash),
            ] {
                registry.set_compiled_selection(id, shapes);
                if let Some(flow) = shapes.flow {
                    registry.set_flow_facts(
                        id,
                        sim::FlowBlockFacts {
                            blocks_motion: flow.blocks_motion,
                            is_solid: flow.is_solid,
                            blocked_faces: flow.blocked_faces,
                            allowed_faces: flow.allowed_faces,
                            liquid_depth: flow.liquid_depth,
                        },
                    );
                }
                if let Some(door) = shapes.door {
                    registry.set_door_state(
                        id,
                        sim::DoorState {
                            family: door.family,
                            facing: [
                                sim::DoorFacing::South,
                                sim::DoorFacing::West,
                                sim::DoorFacing::North,
                                sim::DoorFacing::East,
                            ][usize::from(door.facing)],
                            upper: door.upper,
                            open: door.open,
                            hinge_right: door.hinge_right,
                        },
                    );
                }
                if shapes
                    .flags
                    .contains(assets::block_shapes::ShapeFlags::BAMBOO_OFFSET)
                {
                    registry.set_bamboo_column_offset(id);
                }
                if shapes
                    .flags
                    .contains(assets::block_shapes::ShapeFlags::DRIPSTONE_OFFSET)
                {
                    registry.set_dripstone_column_offset(id);
                }
                if shapes.flags.contains(assets::block_shapes::ShapeFlags::AIR) {
                    registry.set_air_runtime_id(id);
                }
            }
            if record.name.as_ref() != RESERVED_RECORD_NAME {
                let key = protocol::block_name_sort_key(&record.name);
                max_vanilla_sort_key = max_vanilla_sort_key.max(key);
                if vanilla_runs
                    .last()
                    .is_none_or(|run| run.1.as_ref() != record.name.as_ref())
                {
                    vanilla_runs.push((key, Arc::from(record.name.as_ref()), record.sequential_id));
                }
            }
        }
        let available_record_count = physics.len();
        let preg_sha256 = physics.sha256();
        let breg_sha256 = physics.breg_sha256();
        Ok(Self {
            sequential,
            hashed,
            available_record_count,
            sequential_count: physics.len(),
            hashed_count: physics.len(),
            preg_sha256,
            breg_sha256,
            interaction_blocks,
            hashed_interaction_blocks,
            interleave_supported: vanilla_runs
                .windows(2)
                .all(|pair| (pair[0].0, &pair[0].1) < (pair[1].0, &pair[1].1)),
            vanilla_runs,
            canonical_states,
            hashed_canonical_states,
            state_ids,
            max_vanilla_sort_key,
            session_hashes: Vec::new(),
        })
    }

    /// Registers this session's StartGame custom blocks at the sequential ids
    /// after the vanilla palette and returns that id range plus the wire remap
    /// for customs whose names sort among vanilla. Returns `None` when a custom
    /// name equals a vanilla name or the carrier order cannot be interleaved.
    pub fn begin_session_custom_blocks(
        &mut self,
        custom: &protocol::CustomBlocks,
    ) -> Option<(Range<u32>, assets::SequentialIdRemap)> {
        let first = u32::try_from(self.sequential_count).ok()?;
        self.sequential.remove_runtime_ids_from(first);
        self.interaction_blocks.split_off(&first);
        let vanilla_remap = self.admit_server_definitions(custom);
        if custom.blocks.is_empty() {
            return Some((first..first, vanilla_remap));
        }
        let mut next = first;
        let mut runs = Vec::new();
        for block in custom.blocks.iter() {
            let key = (block.sort_key(), block.name.as_ref());
            let after = self
                .vanilla_runs
                .partition_point(|run| (run.0, run.1.as_ref()) <= key);
            let same_name = after
                .checked_sub(1)
                .is_some_and(|index| self.vanilla_runs[index].1.as_ref() == key.1);
            if same_name || (!self.interleave_supported && key.0 <= self.max_vanilla_sort_key) {
                return None;
            }
            let vanilla_before = self.vanilla_runs.get(after).map_or(first, |run| run.2);
            let vanilla_before = vanilla_remap.wire_boundary(vanilla_before);
            let earlier_customs = next - first;
            runs.push((
                vanilla_before.checked_add(earlier_customs)?,
                block.state_count,
                next,
            ));
            if block.visual.materialization_disabled {
                next = next.checked_add(block.state_count)?;
                continue;
            }
            for state in 0..block.state_count {
                let effective = block.physics_for_state(state);
                self.interaction_blocks.insert(
                    next,
                    InteractionBlock {
                        identifier: Arc::clone(&block.name),
                        full_cube: effective.collides && effective.collision_boxes.is_none(),
                        build_intention: false,
                        shape_flags: assets::block_shapes::ShapeFlags::empty(),
                        tags: Arc::clone(&block.tags),
                        connections: None,
                        second_part: None,
                    },
                );
                let boxes = custom_block_boxes(&effective);
                self.sequential
                    .register_with_friction(next, boxes, custom_block_friction(&effective))
                    .ok()?;
                apply_selection(
                    &mut self.sequential,
                    next,
                    effective.selection,
                    effective.transformation,
                );
                if let Some(offset) = effective.random_offset {
                    self.sequential.set_random_offset(next, offset);
                }
                next = next.checked_add(1)?;
            }
        }
        Some((first..next, vanilla_remap.with_insertions(runs)))
    }

    /// Registers this session's custom block states under their network hashes
    /// and returns how many were added; a hash vanilla or an earlier state owns
    /// is skipped.
    pub fn begin_session_hashed_custom_blocks(&mut self, custom: &protocol::CustomBlocks) -> usize {
        for hash in self.session_hashes.drain(..) {
            self.hashed.remove_runtime_id(hash);
            self.hashed_interaction_blocks.remove(&hash);
        }
        for block in custom.blocks.iter() {
            for (index, state) in block.hashed_states().into_iter().enumerate() {
                if self.hashed.contains_runtime_id(state.hash) {
                    continue;
                }
                let effective = block.physics_for_state(index as u32);
                let boxes = custom_block_boxes(&effective);
                if self
                    .hashed
                    .register_with_friction(state.hash, boxes, custom_block_friction(&effective))
                    .is_ok()
                {
                    self.hashed_interaction_blocks.insert(
                        state.hash,
                        InteractionBlock {
                            identifier: Arc::clone(&block.name),
                            full_cube: effective.collides && effective.collision_boxes.is_none(),
                            build_intention: false,
                            shape_flags: assets::block_shapes::ShapeFlags::empty(),
                            tags: Arc::clone(&block.tags),
                            connections: None,
                            second_part: None,
                        },
                    );
                    apply_selection(
                        &mut self.hashed,
                        state.hash,
                        effective.selection,
                        effective.transformation,
                    );
                    if let Some(offset) = effective.random_offset {
                        self.hashed.set_random_offset(state.hash, offset);
                    }
                    self.session_hashes.push(state.hash);
                }
            }
        }
        self.session_hashes.len()
    }

    pub fn block_identifier(&self, mode: assets::NetworkIdMode, runtime_id: u32) -> Option<&str> {
        let map = match mode {
            assets::NetworkIdMode::Sequential => &self.interaction_blocks,
            assets::NetworkIdMode::Hashed => &self.hashed_interaction_blocks,
        };
        map.get(&runtime_id).map(|block| block.identifier.as_ref())
    }

    /// The offset from `runtime_id`'s block to the other block of its two-block structure
    /// (door halves, double plants, pitcher crops, bed ends), if it is part of one.
    pub fn second_part_offset(
        &self,
        mode: assets::NetworkIdMode,
        runtime_id: u32,
    ) -> Option<[i32; 3]> {
        let map = match mode {
            assets::NetworkIdMode::Sequential => &self.interaction_blocks,
            assets::NetworkIdMode::Hashed => &self.hashed_interaction_blocks,
        };
        map.get(&runtime_id)?.second_part
    }

    /// The runtime id of `identifier` in exactly `states`, compared as parsed JSON.
    pub fn block_state_runtime_id(
        &self,
        mode: assets::NetworkIdMode,
        identifier: &str,
        states: &serde_json::Map<String, serde_json::Value>,
    ) -> Option<u32> {
        let key = serde_json::to_string(states).ok()?;
        let &(sequential, hashed) = self.state_ids.get(identifier)?.get(&key)?;
        Some(match mode {
            assets::NetworkIdMode::Sequential => sequential,
            assets::NetworkIdMode::Hashed => hashed,
        })
    }

    /// Whether `runtime_id` is a cube-model block with one full collision box.
    pub fn block_is_full_cube(&self, mode: assets::NetworkIdMode, runtime_id: u32) -> bool {
        let map = match mode {
            assets::NetworkIdMode::Sequential => &self.interaction_blocks,
            assets::NetworkIdMode::Hashed => &self.hashed_interaction_blocks,
        };
        map.get(&runtime_id).is_some_and(|block| block.full_cube)
    }

    /// Borrows the registry facts used by the renderer's neighbor connection predicates.
    pub fn block_connection_facts(
        &self,
        mode: assets::NetworkIdMode,
        runtime_id: u32,
    ) -> Option<(
        assets::ModelFamily,
        assets::ContributorRole,
        bool,
        Option<u32>,
    )> {
        let map = match mode {
            assets::NetworkIdMode::Sequential => &self.interaction_blocks,
            assets::NetworkIdMode::Hashed => &self.hashed_interaction_blocks,
        };
        map.get(&runtime_id)?.connections
    }

    /// The registry's canonical state JSON for `runtime_id`, when it is a registered state.
    pub fn block_canonical_state(
        &self,
        mode: assets::NetworkIdMode,
        runtime_id: u32,
    ) -> Option<&str> {
        let map = match mode {
            assets::NetworkIdMode::Sequential => &self.canonical_states,
            assets::NetworkIdMode::Hashed => &self.hashed_canonical_states,
        };
        map.get(&runtime_id).map(AsRef::as_ref)
    }

    #[must_use]
    pub const fn registry(&self, mode: assets::NetworkIdMode) -> &CollisionRegistry {
        match mode {
            assets::NetworkIdMode::Sequential => &self.sequential,
            assets::NetworkIdMode::Hashed => &self.hashed,
        }
    }

    #[must_use]
    pub const fn registered_count(&self, mode: assets::NetworkIdMode) -> usize {
        match mode {
            assets::NetworkIdMode::Sequential => self.sequential_count,
            assets::NetworkIdMode::Hashed => self.hashed_count,
        }
    }

    #[must_use]
    pub const fn available_record_count(&self) -> usize {
        self.available_record_count
    }

    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.available_record_count != 0
            && self.sequential_count == self.available_record_count
            && self.hashed_count == self.available_record_count
            && self.preg_sha256 != [0; 32]
            && self.breg_sha256 != [0; 32]
    }

    #[must_use]
    pub const fn preg_sha256(&self) -> [u8; 32] {
        self.preg_sha256
    }

    #[must_use]
    pub const fn breg_sha256(&self) -> [u8; 32] {
        self.breg_sha256
    }
}

/// Surface friction of a custom state: one minus its `minecraft:friction` value, else the
/// default block friction. Speed factors and fluid facts stay neutral.
fn custom_block_friction(block: &protocol::CustomBlockPhysics) -> f64 {
    block
        .friction
        .map_or(sim::DEFAULT_SURFACE_FRICTION, |value| {
            f64::from(1.0 - value)
        })
}

/// Yields every authored primitive, or the full-cube default, when collision is enabled.
fn custom_block_boxes(block: &protocol::CustomBlockPhysics) -> impl Iterator<Item = Aabb> + '_ {
    let defaults = block
        .collision_boxes
        .is_none()
        .then(|| Aabb::new(Vec3::ZERO, Vec3::ONE));
    let boxes = block
        .collision_boxes
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|shape| transformed_box(*shape, block.transformation));
    defaults
        .into_iter()
        .chain(boxes.map(box_to_aabb))
        .filter(move |_| block.collides)
}

/// The bounds of one authored box under a state's transformation, so collision and
/// selection follow the same face as the geometry.
fn transformed_box(
    shape: protocol::CustomBox,
    transform: Option<protocol::CustomTransformation>,
) -> protocol::CustomBox {
    let Some(transform) = transform else {
        return shape;
    };
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for bits in 0..8_u32 {
        let corner = [0, 1, 2].map(|axis| {
            if bits >> axis & 1 != 0 {
                shape.max[axis]
            } else {
                shape.min[axis]
            }
        });
        let point = transform
            .apply(corner.map(|coordinate| coordinate * 16.0 - 8.0))
            .map(|coordinate| (coordinate + 8.0) / 16.0);
        for axis in 0..3 {
            min[axis] = min[axis].min(point[axis]);
            max[axis] = max[axis].max(point[axis]);
        }
    }
    protocol::CustomBox { min, max }
}

/// Applies the block's selection box to a registered state's pick ray.
fn apply_selection(
    registry: &mut CollisionRegistry,
    id: u32,
    selection: protocol::CustomSelection,
    transform: Option<protocol::CustomTransformation>,
) {
    let shapes = match selection {
        protocol::CustomSelection::Default => vec![Aabb::new(Vec3::ZERO, Vec3::ONE)],
        protocol::CustomSelection::Disabled => Vec::new(),
        protocol::CustomSelection::Box(shape) => {
            vec![box_to_aabb(transformed_box(shape, transform))]
        }
    };
    registry.set_pick_shapes(id, shapes);
}

fn box_to_aabb(shape: protocol::CustomBox) -> Aabb {
    let point = |values: [f32; 3]| {
        Vec3::new(
            f64::from(values[0]),
            f64::from(values[1]),
            f64::from(values[2]),
        )
    };
    Aabb::new(point(shape.min), point(shape.max))
}

fn collision_box_to_aabb(collision: assets::CollisionBox) -> Aabb {
    let coordinate = |value: i32| f64::from(value) * COLLISION_COORDINATE_SCALE;
    Aabb::new(
        Vec3::new(
            coordinate(collision.min_x),
            coordinate(collision.min_y),
            coordinate(collision.min_z),
        ),
        Vec3::new(
            coordinate(collision.max_x),
            coordinate(collision.max_y),
            coordinate(collision.max_z),
        ),
    )
}

#[cfg(test)]
#[path = "collision_registries/tests.rs"]
mod tests;
