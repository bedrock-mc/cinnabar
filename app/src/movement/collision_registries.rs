//! Runtime-ID collision registries for both Bedrock palette identity modes.
//!
//! Extracted verbatim from the movement `physics` module to respect the
//! per-file architecture line policy; behavior, types, and public paths are
//! unchanged (`crate::movement::PhysicsCollisionRegistries` re-exports this).

use std::path::{Path, PathBuf};
use std::{collections::BTreeMap, ops::Range, sync::Arc};

use assets::RegistryRecord;
use bevy::prelude::Resource;
use sim::{
    Aabb, CollisionIdSpace, CollisionRegistry, CollisionRegistryIdentity, RegistryError, Vec3,
};
use thiserror::Error;

mod connected;
mod doors;
mod scaffolding;
mod selection;
mod stairs;

const COLLISION_COORDINATE_SCALE: f64 = 1.0 / 100_000_000.0;
const FULL_CUBE: assets::CollisionBox = assets::CollisionBox {
    min_x: 0,
    min_y: 0,
    min_z: 0,
    max_x: 100_000_000,
    max_y: 100_000_000,
    max_z: 100_000_000,
};
/// Registry records whose names the carrier deliberately withholds.
const RESERVED_RECORD_NAME: &str = "cinnabar:reserved";

/// Runtime-ID collision registries for both Bedrock palette identity modes.
///
/// The two maps are intentionally distinct: a 32-bit network hash may have
/// the same numeric value as an unrelated sequential ID.
#[derive(Resource, Debug)]
pub struct PhysicsCollisionRegistries {
    sequential: CollisionRegistry,
    hashed: CollisionRegistry,
    available_record_count: usize,
    sequential_count: usize,
    hashed_count: usize,
    preg_sha256: [u8; 32],
    breg_sha256: [u8; 32],
    interaction_blocks: BTreeMap<u32, (Arc<str>, bool)>,
    hashed_interaction_blocks: BTreeMap<u32, (Arc<str>, bool)>,
    /// Named vanilla blocks as `(sort key, name, first sequential id)`, in id order.
    vanilla_runs: Vec<(u64, Arc<str>, u32)>,
    /// False when the carrier's ids are not in vanilla sort order, so customs cannot interleave.
    interleave_supported: bool,
    canonical_states: BTreeMap<u32, Arc<str>>,
    hashed_canonical_states: BTreeMap<u32, Arc<str>>,
    max_vanilla_sort_key: u64,
    custom_block_physics: Option<CustomBlockPhysics>,
    /// Hashes this session added to `hashed`, dropped when the next session begins.
    session_hashes: Vec<u32>,
}

/// Provisional movement facts for server-defined blocks: stone's surface facts.
#[derive(Debug, Clone, Copy)]
struct CustomBlockPhysics {
    friction: f64,
    horizontal_speed: f64,
    vertical_speed: f64,
    flags: u8,
    surface_response: u8,
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
        let mut max_vanilla_sort_key = 0;
        let mut vanilla_runs: Vec<(u64, Arc<str>, u32)> = Vec::new();
        let mut custom_block_physics = None;
        for record in records {
            let fact = physics
                .by_sequential_id(record.sequential_id)
                .expect("strict PREG decoder covers every supplied BREG record");
            let boxes = connected::shapes(record)
                .or_else(|| stairs::shapes(record))
                .or_else(|| scaffolding::shapes(record))
                .unwrap_or_else(|| {
                    fact.boxes
                        .iter()
                        .copied()
                        .map(collision_box_to_aabb)
                        .collect::<Vec<_>>()
                });
            let full_cube = record.model_family == assets::ModelFamily::Cube
                && fact.boxes.len() == 1
                && fact.boxes[0] == FULL_CUBE;
            let binding = (Arc::from(record.name.as_ref()), full_cube);
            interaction_blocks.insert(record.sequential_id, binding.clone());
            hashed_interaction_blocks.insert(record.network_hash, binding);
            let state: Arc<str> = Arc::from(record.canonical_state.as_ref());
            canonical_states.insert(record.sequential_id, Arc::clone(&state));
            hashed_canonical_states.insert(record.network_hash, state);
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
            if let Some(shape) = selection::shape(record) {
                sequential.set_pick_shapes(record.sequential_id, [shape]);
                hashed.set_pick_shapes(record.network_hash, [shape]);
            }
            if let Some(door) = doors::state(record) {
                sequential.set_door_state(record.sequential_id, door.clone());
                hashed.set_door_state(record.network_hash, door);
            }
            if record.name.as_ref() == "minecraft:air" {
                sequential.set_air_runtime_id(record.sequential_id);
                hashed.set_air_runtime_id(record.network_hash);
            }
            if record.name.as_ref() == "minecraft:stone" {
                custom_block_physics.get_or_insert(CustomBlockPhysics {
                    friction: f64::from(fact.friction_q1e8) * COLLISION_COORDINATE_SCALE,
                    horizontal_speed: f64::from(fact.horizontal_speed_q1e8)
                        * COLLISION_COORDINATE_SCALE,
                    vertical_speed: f64::from(fact.vertical_speed_q1e8)
                        * COLLISION_COORDINATE_SCALE,
                    flags: fact.flags.bits(),
                    surface_response: fact.surface_response as u8,
                });
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
            max_vanilla_sort_key,
            custom_block_physics,
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
        if custom.blocks.is_empty() {
            return Some((first..first, assets::SequentialIdRemap::default()));
        }
        let physics = self.custom_block_physics?;
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
            let earlier_customs = next - first;
            runs.push((vanilla_before + earlier_customs, block.state_count, next));
            let binding = (
                Arc::clone(&block.name),
                block.collides && block.collision_box.is_none(),
            );
            for _ in 0..block.state_count {
                self.interaction_blocks.insert(next, binding.clone());
                let boxes = custom_block_box(block);
                self.sequential
                    .register_primitives(
                        next,
                        boxes,
                        physics.friction,
                        physics.horizontal_speed,
                        physics.vertical_speed,
                        0.0,
                        physics.flags,
                        physics.surface_response,
                    )
                    .ok()?;
                apply_selection(&mut self.sequential, next, block);
                next = next.checked_add(1)?;
            }
        }
        // Customs sorting after every vanilla name need no remap.
        let identity = runs
            .iter()
            .all(|&(wire_start, _, internal_start)| wire_start == internal_start);
        let remap = if identity {
            assets::SequentialIdRemap::default()
        } else {
            assets::SequentialIdRemap::new(runs)
        };
        Some((first..next, remap))
    }

    /// Registers this session's custom block states under their network hashes
    /// and returns how many were added; a hash vanilla or an earlier state owns
    /// is skipped. Returns `None` when the carrier lacks stone physics to borrow.
    pub fn begin_session_hashed_custom_blocks(
        &mut self,
        custom: &protocol::CustomBlocks,
    ) -> Option<usize> {
        for hash in self.session_hashes.drain(..) {
            self.hashed.remove_runtime_id(hash);
            self.hashed_interaction_blocks.remove(&hash);
        }
        if custom.blocks.is_empty() {
            return Some(0);
        }
        let physics = self.custom_block_physics?;
        for block in custom.blocks.iter() {
            for state in block.hashed_states() {
                if self.hashed.contains_runtime_id(state.hash) {
                    continue;
                }
                let boxes = custom_block_box(block);
                if self
                    .hashed
                    .register_primitives(
                        state.hash,
                        boxes,
                        physics.friction,
                        physics.horizontal_speed,
                        physics.vertical_speed,
                        0.0,
                        physics.flags,
                        physics.surface_response,
                    )
                    .is_ok()
                {
                    self.hashed_interaction_blocks.insert(
                        state.hash,
                        (
                            Arc::clone(&block.name),
                            block.collides && block.collision_box.is_none(),
                        ),
                    );
                    apply_selection(&mut self.hashed, state.hash, block);
                    self.session_hashes.push(state.hash);
                }
            }
        }
        Some(self.session_hashes.len())
    }

    pub(crate) fn block_identifier(
        &self,
        mode: assets::NetworkIdMode,
        runtime_id: u32,
    ) -> Option<&str> {
        let map = match mode {
            assets::NetworkIdMode::Sequential => &self.interaction_blocks,
            assets::NetworkIdMode::Hashed => &self.hashed_interaction_blocks,
        };
        map.get(&runtime_id)
            .map(|(identifier, _)| identifier.as_ref())
    }

    /// The runtime id of `identifier` in exactly `states`, compared as parsed JSON.
    pub(crate) fn block_state_runtime_id(
        &self,
        mode: assets::NetworkIdMode,
        identifier: &str,
        states: &serde_json::Map<String, serde_json::Value>,
    ) -> Option<u32> {
        let (names, canonical) = match mode {
            assets::NetworkIdMode::Sequential => (&self.interaction_blocks, &self.canonical_states),
            assets::NetworkIdMode::Hashed => (
                &self.hashed_interaction_blocks,
                &self.hashed_canonical_states,
            ),
        };
        names
            .iter()
            .filter(|(_, (name, _))| name.as_ref() == identifier)
            .find(|(runtime_id, _)| {
                canonical.get(runtime_id).is_some_and(|state| {
                    serde_json::from_str::<serde_json::Map<_, _>>(state)
                        .is_ok_and(|parsed| &parsed == states)
                })
            })
            .map(|(runtime_id, _)| *runtime_id)
    }

    /// Whether `runtime_id` is a cube-model block with one full collision box.
    pub(crate) fn block_is_full_cube(&self, mode: assets::NetworkIdMode, runtime_id: u32) -> bool {
        let map = match mode {
            assets::NetworkIdMode::Sequential => &self.interaction_blocks,
            assets::NetworkIdMode::Hashed => &self.hashed_interaction_blocks,
        };
        map.get(&runtime_id)
            .is_some_and(|(_, full_cube)| *full_cube)
    }

    /// The registry's canonical state JSON for `runtime_id`, when it is a registered state.
    pub(crate) fn block_canonical_state(
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

/// The block's collision shape: none when disabled, else its box or a full cube.
fn custom_block_box(block: &protocol::CustomBlock) -> Option<Aabb> {
    if !block.collides {
        return None;
    }
    Some(
        block
            .collision_box
            .map_or_else(|| collision_box_to_aabb(FULL_CUBE), box_to_aabb),
    )
}

/// Applies the block's selection box to a registered state's pick ray.
fn apply_selection(registry: &mut CollisionRegistry, id: u32, block: &protocol::CustomBlock) {
    let shapes = match block.selection {
        protocol::CustomSelection::Default => return,
        protocol::CustomSelection::Disabled => Vec::new(),
        protocol::CustomSelection::Box(shape) => vec![box_to_aabb(shape)],
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
mod tests {
    use std::path::{Path, PathBuf};

    use sha2::{Digest, Sha256};

    use super::{PhysicsCollisionRegistries, PhysicsCollisionRegistryError};
    use crate::asset_startup::active_content_registry_protocol;

    const BREG_V1001: &[u8] =
        include_bytes!("../../../crates/assets/data/block-registry-v1001.bin");
    const BREG_V2193: &[u8] =
        include_bytes!("../../../crates/assets/data/block-registry-v2193.bin");

    /// Minimal byte-valid PREG stamped for one protocol and bound to one BREG
    /// digest; shape mirrors the committed movement fixtures so no new
    /// artifact is required.
    fn synthetic_preg(protocol: u32, breg: &[u8], records: &[assets::RegistryRecord]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"PREG1001");
        bytes.extend_from_slice(&protocol.to_le_bytes());
        bytes.extend_from_slice(&u32::try_from(records.len()).unwrap().to_le_bytes());
        bytes.extend_from_slice(&Sha256::digest(breg));
        for record in records {
            bytes.extend_from_slice(&record.sequential_id.to_le_bytes());
            bytes.extend_from_slice(&record.network_hash.to_le_bytes());
            bytes.push(u8::try_from(record.collision_seed.boxes.len()).unwrap());
            bytes.push(if record.collision_seed.boxes.is_empty() {
                assets::BlockPhysicsFlags::PASSABLE.bits()
            } else {
                0
            });
            bytes.extend_from_slice(&[0, 0]);
            bytes.extend_from_slice(&60_000_000_u32.to_le_bytes());
            bytes.extend_from_slice(&100_000_000_u32.to_le_bytes());
            bytes.extend_from_slice(&100_000_000_u32.to_le_bytes());
            bytes.extend_from_slice(&0_i32.to_le_bytes());
            for shape in &record.collision_seed.boxes {
                for coordinate in [
                    shape.min_x,
                    shape.min_y,
                    shape.min_z,
                    shape.max_x,
                    shape.max_y,
                    shape.max_z,
                ] {
                    bytes.extend_from_slice(&coordinate.to_le_bytes());
                }
            }
        }
        let digest = Sha256::digest(&bytes);
        bytes.extend_from_slice(&digest);
        bytes
    }

    fn bind(
        breg: &[u8],
        preg: &[u8],
        expected_protocol: u32,
    ) -> Result<PhysicsCollisionRegistries, PhysicsCollisionRegistryError> {
        PhysicsCollisionRegistries::bind_coherent_assets(
            breg,
            preg,
            Path::new("installed/physics/block-physics.bin"),
            Path::new("installed/assets/world.mcbea"),
            expected_protocol,
        )
    }

    fn custom_block(name: &str, state_count: u32) -> protocol::CustomBlock {
        protocol::CustomBlock {
            name: name.into(),
            state_count,
            collides: true,
            collision_box: None,
            selection: Default::default(),
            visual: Default::default(),
        }
    }

    /// Non-colliding plants must still be targets for the first punch.
    #[test]
    fn noncolliding_plants_have_vanilla_pick_shapes_in_both_id_spaces() {
        let protocol = active_content_registry_protocol();
        let records = assets::read_registry_for_protocol(BREG_V2193, protocol).unwrap();
        let registries = bind(
            BREG_V2193,
            &synthetic_preg(protocol, BREG_V2193, &records),
            protocol,
        )
        .unwrap();
        for name in [
            "minecraft:short_grass",
            "minecraft:tall_grass",
            "minecraft:dandelion",
            "minecraft:poppy",
            "minecraft:oak_sapling",
            "minecraft:deadbush",
            "minecraft:torch",
            "minecraft:soul_torch",
            "minecraft:redstone_torch",
            "minecraft:unlit_redstone_torch",
            "minecraft:short_dry_grass",
            "minecraft:brown_mushroom",
            "minecraft:red_mushroom",
            "minecraft:nether_sprouts",
            "minecraft:cactus_flower",
            "minecraft:reeds",
            "minecraft:wheat",
            "minecraft:carrots",
            "minecraft:potatoes",
            "minecraft:beetroot",
            "minecraft:nether_wart",
        ] {
            let matched = records
                .iter()
                .filter(|record| record.name.as_ref() == name)
                .collect::<Vec<_>>();
            assert!(!matched.is_empty(), "{name} must exist in the carrier");
            for record in matched {
                assert!(record.collision_seed.boxes.is_empty(), "{name} is passable");
                for (mode, id) in [
                    (assets::NetworkIdMode::Sequential, record.sequential_id),
                    (assets::NetworkIdMode::Hashed, record.network_hash),
                ] {
                    assert!(
                        !registries
                            .registry(mode)
                            .selection_shapes(id)
                            .unwrap()
                            .is_empty(),
                        "{name} has a visual pick shape despite no movement collision"
                    );
                    let air_record = records
                        .iter()
                        .find(|record| record.name.as_ref() == "minecraft:air")
                        .unwrap();
                    let air = match mode {
                        assets::NetworkIdMode::Sequential => air_record.sequential_id,
                        assets::NetworkIdMode::Hashed => air_record.network_hash,
                    };
                    let mut store = world::ChunkStore::new();
                    // The pick ray inspects a halo for connected/protruding shapes.
                    for x in -1..=1 {
                        for z in -1..=1 {
                            store
                                .mark_chunk_loaded(world::ChunkKey::new(0, x, z))
                                .unwrap();
                        }
                    }
                    let key = world::SubChunkKey::new(0, 0, 0, 0);
                    store
                        .update_block(key, world::BlockUpdate::new(0, 0, 2, 0, id), air)
                        .unwrap();
                    let shape = registries.registry(mode).selection_shapes(id).unwrap()[0];
                    let origin = sim::Vec3::new(
                        (shape.min.x + shape.max.x) * 0.5,
                        (shape.min.y + shape.max.y) * 0.5,
                        0.5,
                    );
                    let hit = sim::PaletteWorld::new(&store, registries.registry(mode), 0)
                        .block_interaction_ray_current(origin, sim::Vec3::new(0.0, 0.0, 1.0), 3.0)
                        .unwrap()
                        .expect("a first punch can pick the plant");
                    assert_eq!((hit.block_pos, hit.runtime_id), ([0, 0, 2], id));
                }
            }
        }
    }

    /// Custom states take the ids after vanilla, reset per session, and refuse id shifts.
    #[test]
    fn session_custom_blocks_append_after_vanilla_ids() {
        let records =
            assets::read_registry_for_protocol(BREG_V2193, active_content_registry_protocol())
                .unwrap();
        let preg = synthetic_preg(active_content_registry_protocol(), BREG_V2193, &records);
        let mut registries = bind(BREG_V2193, &preg, active_content_registry_protocol()).unwrap();
        let first = u32::try_from(records.len()).unwrap();
        let appended = protocol::CustomBlocks {
            blocks: vec![
                custom_block("lifeboat:lucky_block_9nnvjzz", 1),
                custom_block("lifeboat:coal_ore_generator_a451ess", 4),
            ]
            .into(),
            skipped: 0,
        };
        let (range, remap) = registries.begin_session_custom_blocks(&appended).unwrap();
        assert_eq!(range, first..first + 5);
        assert!(remap.is_identity(), "customs after vanilla keep wire ids");
        assert_eq!(
            registries
                .begin_session_custom_blocks(&protocol::CustomBlocks::default())
                .map(|(range, _)| range),
            Some(first..first)
        );
        let interleaved = protocol::CustomBlocks {
            blocks: vec![custom_block("minecraft:stone", 1)].into(),
            skipped: 0,
        };
        assert_eq!(registries.begin_session_custom_blocks(&interleaved), None);
        // A name sorting among vanilla takes the wire ids from the first vanilla state after
        // it, so every later vanilla wire id is one more than the carrier's.
        let among = protocol::CustomBlocks {
            blocks: vec![custom_block("benergistics:controller", 1)].into(),
            skipped: 0,
        };
        let (range, remap) = registries.begin_session_custom_blocks(&among).unwrap();
        assert_eq!(range, first..first + 1);
        let key = (
            protocol::block_name_sort_key("benergistics:controller"),
            "benergistics:controller",
        );
        let state = |name: &str| {
            records
                .iter()
                .find(|record| record.name.as_ref() == name)
                .unwrap()
                .sequential_id
        };
        let wire = records
            .iter()
            .filter(|record| {
                (
                    protocol::block_name_sort_key(&record.name),
                    record.name.as_ref(),
                ) > key
            })
            .map(|record| record.sequential_id)
            .min()
            .unwrap();
        assert_eq!(remap.to_internal(wire), first);
        let (air, dirt) = (state("minecraft:air"), state("minecraft:dirt"));
        assert!(dirt < wire && wire <= air);
        assert_eq!(remap.to_internal(dirt), dirt);
        assert_eq!(remap.to_internal(air + 1), air);
        assert_eq!(remap.to_internal(first), first - 1);
    }

    /// Hashed custom states register under their hashes and are dropped by the next session.
    #[test]
    fn session_hashed_custom_blocks_register_and_reset() {
        let records =
            assets::read_registry_for_protocol(BREG_V2193, active_content_registry_protocol())
                .unwrap();
        let preg = synthetic_preg(active_content_registry_protocol(), BREG_V2193, &records);
        let mut registries = bind(BREG_V2193, &preg, active_content_registry_protocol()).unwrap();
        let custom = protocol::CustomBlocks {
            blocks: vec![custom_block("test:hashed", 1)].into(),
            skipped: 0,
        };
        let hash = custom.blocks[0].hashed_states()[0].hash;
        assert_eq!(
            registries.begin_session_hashed_custom_blocks(&custom),
            Some(1)
        );
        assert!(registries.hashed.contains_runtime_id(hash));
        assert_eq!(
            registries.begin_session_hashed_custom_blocks(&protocol::CustomBlocks::default()),
            Some(0)
        );
        assert!(!registries.hashed.contains_runtime_id(hash));
    }

    /// The live LBSG aliasing mechanism: a byte-valid PREG whose stamped
    /// header protocol disagrees with the active authority must fail closed
    /// through the dedicated typed error naming both protocols and both
    /// artifact paths. Before this gate existed the same input surfaced only
    /// the generic legacy detail string ("protocol is not 1001") with no path
    /// attribution.
    #[test]
    fn valid_wrong_protocol_preg_fails_with_typed_cross_carrier_mismatch() {
        let legacy_records = assets::read_registry(BREG_V1001).unwrap();
        let preg_v1001 = synthetic_preg(1001, BREG_V1001, &legacy_records);
        let error = bind(BREG_V2193, &preg_v1001, active_content_registry_protocol())
            .expect_err("a flipped physics registry must fail startup");

        let PhysicsCollisionRegistryError::ProtocolMismatch {
            expected_protocol,
            actual_protocol,
            physics_registry_path,
            world_carrier_path,
        } = &error
        else {
            panic!("expected ProtocolMismatch, got {error:?}");
        };
        assert_eq!(*expected_protocol, 2193);
        assert_eq!(*actual_protocol, 1001);
        assert_eq!(
            physics_registry_path,
            &PathBuf::from("installed/physics/block-physics.bin")
        );
        assert_eq!(
            world_carrier_path,
            &PathBuf::from("installed/assets/world.mcbea")
        );
        let message = format!("{error}");
        assert!(
            message.contains("1001") && message.contains("2193"),
            "{message}"
        );
        assert!(message.contains("block-physics.bin"), "{message}");
        assert!(message.contains("world.mcbea"), "{message}");
    }

    /// Accepted production path: both carriers carry the active protocol.
    #[test]
    fn coherent_active_protocol_pair_binds_completely() {
        let registries = bind(
            BREG_V2193,
            &synthetic_preg(
                2193,
                BREG_V2193,
                &assets::read_registry_for_protocol(BREG_V2193, 2193)
                    .expect("checked-in v2193 BREG"),
            ),
            active_content_registry_protocol(),
        )
        .expect("the pinned protocol pair must bind");

        assert!(registries.is_complete());
        assert!(registries.available_record_count() > 0);
    }

    /// Consolidation witness: driving the seam with a mutated authority value
    /// flips its acceptance decision on identical bytes, so both gates'
    /// expectations provably hang off the one shared knob instead of local
    /// constants.
    #[test]
    fn mutating_the_authority_flips_the_binding_decision() {
        let accepted = bind(
            BREG_V1001,
            &synthetic_preg(
                1001,
                BREG_V1001,
                &assets::read_registry_for_protocol(BREG_V1001, 1001)
                    .expect("checked-in v1001 BREG"),
            ),
            1001,
        )
        .is_ok();
        let rejected = bind(
            BREG_V1001,
            &synthetic_preg(
                1001,
                BREG_V1001,
                &assets::read_registry_for_protocol(BREG_V1001, 1001)
                    .expect("checked-in v1001 BREG"),
            ),
            2193,
        )
        .is_err();

        assert!(accepted && rejected);
    }
}
