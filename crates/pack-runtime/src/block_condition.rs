//! Block-state queries shared by independently prepared component domains.

use pack_entity_compiler::{BlockMolang, BlockStateValue};
use protocol::{CustomBlock, CustomStateValue};

/// Evaluates a prepared condition against the named values of one admitted palette state.
pub(super) fn evaluate(
    condition: &BlockMolang,
    block: &CustomBlock,
    values: &[CustomStateValue],
) -> Option<f32> {
    condition
        .evaluate(&|name| {
            let index = block
                .visual
                .state_axes
                .iter()
                .position(|axis| axis.name.as_ref() == name)?;
            Some(match &values[index] {
                CustomStateValue::String(value) => BlockStateValue::String(value),
                CustomStateValue::Int(value) => BlockStateValue::Number(*value as f32),
                CustomStateValue::Bool(value) => BlockStateValue::Number(f32::from(*value)),
            })
        })
        .filter(|value| value.is_finite())
}
