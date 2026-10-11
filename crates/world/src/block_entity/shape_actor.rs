//! Actor inputs decoded once, before collision and interaction queries.

use crate::block_state_identity::{StateValue, state_identity};
use crate::{BlockEntityNbt, NbtValue};

#[cfg(test)]
mod tests;

/// Typed state needed to resolve piston-driven clipping geometry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ShapeActor {
    #[default]
    None,
    Piston {
        progress_bits: u32,
    },
    Moving {
        state_identity: [u8; 32],
        piston: [i32; 3],
        expanding: bool,
    },
}

impl ShapeActor {
    /// Projects admitted actor NBT into finite geometry inputs without retaining a parsed tree.
    pub(super) fn from_nbt(nbt: &BlockEntityNbt) -> Self {
        if !matches!(nbt.id(), Some("PistonArm" | "MovingBlock")) {
            return Self::None;
        }
        let Some(root) = nbt.parse() else {
            return Self::None;
        };
        if nbt.id() == Some("PistonArm") {
            let progress = match root.get("Progress") {
                Some(NbtValue::Float(value)) => *value,
                _ => 0.0,
            };
            return if progress.is_finite() && (0.0..=1.0).contains(&progress) {
                Self::Piston {
                    progress_bits: progress.to_bits(),
                }
            } else {
                Self::None
            };
        }
        let Some(block) = root.compound("movingBlock") else {
            return Self::None;
        };
        let Some(name) = block.string("name") else {
            return Self::None;
        };
        let Some(states) = block.compound("states") else {
            return Self::None;
        };
        let Some(values) = states
            .iter()
            .map(|(key, value)| match value {
                NbtValue::Byte(value) => Some((key, StateValue::Byte(*value))),
                NbtValue::Int(value) => Some((key, StateValue::Int(*value))),
                NbtValue::String(value) => Some((key, StateValue::String(value))),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
        else {
            return Self::None;
        };
        let piston = ["pistonPosX", "pistonPosY", "pistonPosZ"].map(|key| match root.get(key) {
            Some(NbtValue::Int(value)) => *value,
            _ => 0,
        });
        Self::Moving {
            state_identity: state_identity(name, values),
            piston,
            expanding: matches!(root.get("expanding"), Some(NbtValue::Byte(value)) if *value != 0),
        }
    }
}
