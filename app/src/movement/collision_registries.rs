//! Runtime-ID collision registries for both Bedrock palette identity modes.
//!
//! Extracted verbatim from the movement `physics` module to respect the
//! per-file architecture line policy; behavior, types, and public paths are
//! unchanged (`crate::movement::PhysicsCollisionRegistries` re-exports this).

use std::path::{Path, PathBuf};

use assets::RegistryRecord;
use bevy::prelude::Resource;
use sim::{
    Aabb, CollisionIdSpace, CollisionRegistry, CollisionRegistryIdentity, RegistryError, Vec3,
};
use thiserror::Error;

const COLLISION_COORDINATE_SCALE: f64 = 1.0 / 100_000_000.0;

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
    /// flip (world 2168 + physics 1001 or reverse) would recreate the live
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
        for record in records {
            let fact = physics
                .by_sequential_id(record.sequential_id)
                .expect("strict PREG decoder covers every supplied BREG record");
            let boxes = fact
                .boxes
                .iter()
                .copied()
                .map(collision_box_to_aabb)
                .collect::<Vec<_>>();
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
            if record.name.as_ref() == "minecraft:air" {
                sequential.set_air_runtime_id(record.sequential_id);
                hashed.set_air_runtime_id(record.network_hash);
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
        })
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
    const BREG_V2168: &[u8] =
        include_bytes!("../../../crates/assets/data/block-registry-v2168.bin");
    const PREG_V2168: &[u8] = include_bytes!("../../../crates/assets/data/block-physics-v2168.bin");

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

    /// The live LBSG aliasing mechanism: a byte-valid PREG whose stamped
    /// header protocol disagrees with the active authority must fail closed
    /// through the dedicated typed error naming both protocols and both
    /// artifact paths. Before this gate existed the same input surfaced only
    /// the generic legacy detail string ("protocol is not 1001") with no path
    /// attribution.
    #[test]
    fn valid_wrong_protocol_preg_fails_with_typed_cross_carrier_mismatch() {
        let error = bind(BREG_V1001, PREG_V2168, active_content_registry_protocol())
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
        assert_eq!(*expected_protocol, 1001);
        assert_eq!(*actual_protocol, 2168);
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
            message.contains("1001") && message.contains("2168"),
            "{message}"
        );
        assert!(message.contains("block-physics.bin"), "{message}");
        assert!(message.contains("world.mcbea"), "{message}");
    }

    /// Accepted production path: both carriers carry the active protocol.
    #[test]
    fn coherent_active_protocol_pair_binds_completely() {
        let registries = bind(
            BREG_V1001,
            &synthetic_preg(
                1001,
                BREG_V1001,
                &assets::read_registry_for_protocol(BREG_V1001, 1001)
                    .expect("checked-in v1001 BREG"),
            ),
            active_content_registry_protocol(),
        )
        .expect("the pinned protocol pair must bind");

        assert!(registries.is_complete());
        assert!(registries.available_record_count() > 0);
    }

    /// Synthetic both-2168 acceptance at the pure binding seam: flipping the
    /// single authority accepts a matching committed v2168 carrier pair
    /// without any new artifact, proving the gate tracks the authority rather
    /// than a second hidden literal.
    #[test]
    fn coherent_flipped_authority_pair_is_accepted_at_the_binding_seam() {
        let records =
            assets::read_registry_for_protocol(BREG_V2168, 2168).expect("checked-in v2168 BREG");
        let registries = bind(
            BREG_V2168,
            &synthetic_preg(2168, BREG_V2168, &records),
            2168,
        )
        .expect("a matched flipped pair must bind under the flipped expectation");

        assert!(registries.is_complete());
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
            2168,
        )
        .is_err();

        assert!(accepted && rejected);
    }
}
