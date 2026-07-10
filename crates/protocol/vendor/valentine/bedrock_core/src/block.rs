//! Core block traits for the Flyweight-Static Registry Pattern.
//!
//! These traits are defined here in bedrock_core.
//! The actual implementations (ZST blocks, state structs) are GENERATED
//! by valentine_gen into the version crates.

use std::fmt::{self, Debug, Display};

/// Error returned when constructing a block state with invalid values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateError {
    /// A numeric field is out of valid range.
    OutOfRange {
        field: &'static str,
        value: u32,
        min: u32,
        max: u32,
    },
    /// An enum field has an invalid variant.
    InvalidVariant { field: &'static str, value: u32 },
}

impl Display for StateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfRange {
                field,
                value,
                min,
                max,
            } => {
                write!(
                    f,
                    "state field '{}' value {} out of range [{}, {}]",
                    field, value, min, max
                )
            }
            Self::InvalidVariant { field, value } => {
                write!(f, "state field '{}' has invalid variant {}", field, value)
            }
        }
    }
}

impl std::error::Error for StateError {}

/// Trait for block state types.
///
/// Stateless blocks use `()` which implements this trait.
/// Stateful blocks use generated state structs (DoorState, StairState, etc.)
pub trait BlockState: Copy + Default + Debug + Send + Sync + 'static {
    /// Compute the state offset from the block's min_state_id.
    fn state_offset(&self) -> u32;

    /// Create a state from a raw offset. Returns None if invalid.
    fn from_offset(offset: u32) -> Option<Self>;

    /// Number of valid states.
    fn state_count() -> u32;
}

impl BlockState for () {
    fn state_offset(&self) -> u32 {
        0
    }
    fn from_offset(offset: u32) -> Option<Self> {
        (offset == 0).then_some(())
    }
    fn state_count() -> u32 {
        1
    }
}

/// Trait for block definitions. Implemented by zero-sized marker types.
///
/// Each block (Stone, AcaciaDoor, etc.) is a ZST implementing this trait.
/// All data is const for zero-cost access.
pub trait BlockDef: 'static + Send + Sync + Sized {
    const ID: u32;
    /// Raw block string ID from protocol (e.g., "minecraft:stone")
    const STRING_ID: &'static str;
    /// Display name (e.g., "Stone", "Acacia Door")
    const NAME: &'static str;
    const HARDNESS: f32;
    const RESISTANCE: f32;
    const IS_TRANSPARENT: bool;
    const EMIT_LIGHT: u8;
    const FILTER_LIGHT: u8;
    const MIN_STATE_ID: u32;
    const MAX_STATE_ID: u32;

    /// Associated state type. `()` for stateless blocks.
    type State: BlockState;

    /// Default state for this block.
    fn default_state() -> Self::State;

    /// Compute runtime state ID from a state.
    #[inline]
    fn state_id(state: &Self::State) -> u32 {
        Self::MIN_STATE_ID + state.state_offset()
    }

    /// Get state from a runtime state ID.
    #[inline]
    fn state_from_id(state_id: u32) -> Option<Self::State> {
        if state_id < Self::MIN_STATE_ID || state_id > Self::MAX_STATE_ID {
            return None;
        }
        Self::State::from_offset(state_id - Self::MIN_STATE_ID)
    }
}

/// Object-safe trait for dynamic block lookups.
/// Automatically implemented for all BlockDef types.
pub trait BlockDefDyn: Send + Sync + 'static {
    fn id(&self) -> u32;
    fn string_id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn hardness(&self) -> f32;
    fn resistance(&self) -> f32;
    fn is_transparent(&self) -> bool;
    fn emit_light(&self) -> u8;
    fn filter_light(&self) -> u8;
    fn min_state_id(&self) -> u32;
    fn max_state_id(&self) -> u32;
    fn state_count(&self) -> u32;
    fn default_state_id(&self) -> u32;
}

impl<T: BlockDef> BlockDefDyn for T {
    fn id(&self) -> u32 {
        T::ID
    }
    fn string_id(&self) -> &'static str {
        T::STRING_ID
    }
    fn name(&self) -> &'static str {
        T::NAME
    }
    fn hardness(&self) -> f32 {
        T::HARDNESS
    }
    fn resistance(&self) -> f32 {
        T::RESISTANCE
    }
    fn is_transparent(&self) -> bool {
        T::IS_TRANSPARENT
    }
    fn emit_light(&self) -> u8 {
        T::EMIT_LIGHT
    }
    fn filter_light(&self) -> u8 {
        T::FILTER_LIGHT
    }
    fn min_state_id(&self) -> u32 {
        T::MIN_STATE_ID
    }
    fn max_state_id(&self) -> u32 {
        T::MAX_STATE_ID
    }
    fn state_count(&self) -> u32 {
        T::State::state_count()
    }
    fn default_state_id(&self) -> u32 {
        T::state_id(&T::default_state())
    }
}
