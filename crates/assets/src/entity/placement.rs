//! Placement and target-rotation frames admitted with a rig, independent of presentation names.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{CompiledEntityAssets, EntityAssetKind, invalid};
use crate::AssetError;

pub const SOURCE_PATH: &str = "registry/entity-placement-frame.json";
pub const SOURCE_BYTES: &[u8] = crate::facts::entity::SOURCE_BYTES;

/// Coordinate frame used by the rig's placement or target-rotation query.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ActorYawFrame {
    Body,
    Absolute,
}

/// Missing class coverage remains distinct from an admitted ordinary body frame.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum EntityPlacementFrame {
    #[default]
    Unresolved,
    /// Retains existing routing while complete class and component coverage remains open.
    Provisional {
        placement: ActorYawFrame,
        target_rotation: ActorYawFrame,
    },
}

impl EntityPlacementFrame {
    /// Returns the placement contract, leaving unsupported identities unresolved.
    pub const fn placement(self) -> Option<ActorYawFrame> {
        match self {
            Self::Unresolved => None,
            Self::Provisional { placement, .. } => Some(placement),
        }
    }

    /// Returns the query contract independently from the placement transform.
    pub const fn target_rotation(self) -> Option<ActorYawFrame> {
        match self {
            Self::Unresolved => None,
            Self::Provisional {
                target_rotation, ..
            } => Some(target_rotation),
        }
    }
}

/// Resolves a compiled placement disposition during rig admission.
pub fn builtin_placement_frame(identifier: &str) -> EntityPlacementFrame {
    crate::facts::entity::EntityFacts::pinned()
        .get(identifier)
        .placement
}

/// Checks every independently collected rig identity against its hash-bound frame source.
pub(super) fn validate(compiled: &CompiledEntityAssets) -> Result<(), AssetError> {
    let source = compiled
        .sources
        .iter()
        .find(|source| source.path.as_ref() == SOURCE_PATH);
    if source.is_some_and(|source| {
        source.source_bytes as usize != SOURCE_BYTES.len()
            || source.source_sha256 != <[u8; 32]>::from(Sha256::digest(SOURCE_BYTES))
    }) {
        return Err(invalid("placement frame source identity changed"));
    }
    for rig in &compiled.rig_bindings {
        let symbol = compiled
            .symbols
            .get(rig.entity_symbol as usize)
            .ok_or_else(|| invalid("placement frame references an absent entity"))?;
        let expected = if source.is_some() && symbol.kind == EntityAssetKind::Entity {
            builtin_placement_frame(&symbol.identifier)
        } else {
            EntityPlacementFrame::Unresolved
        };
        if rig.placement_frame != expected {
            return Err(invalid(
                "rig placement frame disagrees with its admitted identity",
            ));
        }
    }
    Ok(())
}
