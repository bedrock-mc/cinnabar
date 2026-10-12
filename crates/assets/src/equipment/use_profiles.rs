//! Authored item-use overlays compiled independently from item duration inference.

use crate::{
    AssetError,
    item_use::{ItemConsumption, ItemUseCategory},
};
use serde::{Deserialize, Serialize};

/// Explicit component facts; absent fields do not erase the compiled base admission.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompiledItemUseOverlay {
    pub identifier: Box<str>,
    pub duration_ticks: Option<u32>,
    pub food_can_always_eat: Option<bool>,
    pub consumption: Option<ItemConsumption>,
    pub movement_modifier_bits: Option<u64>,
    pub cooldown: Option<ItemUseCategory>,
}

/// Rejects malformed optional facts and duplicate identities before numerical admission.
pub(super) fn validate(entries: &[CompiledItemUseOverlay]) -> Result<(), AssetError> {
    if entries.len() > super::MAX_ITEM_TIMINGS {
        return Err(super::invalid("item-use overlay count exceeds bound"));
    }
    let mut previous: Option<&str> = None;
    for entry in entries {
        super::validate_identifier(&entry.identifier)?;
        if previous.is_some_and(|name| name >= entry.identifier.as_ref())
            || entry.movement_modifier_bits.is_some_and(|bits| {
                let value = f64::from_bits(bits);
                !value.is_finite() || value < 0.0
            })
            || entry.cooldown.as_ref().is_some_and(|cooldown| {
                cooldown.category.is_empty()
                    || cooldown.category.len() > super::MAX_EQUIPMENT_IDENTIFIER_BYTES
            })
        {
            return Err(super::invalid("invalid or unordered item-use overlay"));
        }
        previous = Some(&entry.identifier);
    }
    Ok(())
}
