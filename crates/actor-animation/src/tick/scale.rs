//! Owner body and main-hand scales share the bounded animation evaluator.
use super::*;

/// Scale scripts share the variable and random stream that later animation channels read.
pub(crate) fn evaluate_model(
    evaluator: &Evaluator<'_>,
    rig: &assets::EntityRigBinding,
    variables: &mut MolangVariables,
    budget: &mut EvalBudget<'_>,
) -> Result<Option<[f32; 4]>, EvalError> {
    let Some(expressions) = rig.scale_expressions else {
        return Ok(None);
    };
    let mut scale = [1.0; 4];
    for (slot, expression) in scale.iter_mut().zip(expressions) {
        let value = evaluator.number(expression as usize, variables, 1.0, budget)?;
        *slot = if value.is_finite() { value } else { 1.0 };
    }
    Ok(Some(scale))
}

/// Evaluates the optional positive main-hand scale; invalid results retain unit size.
pub(crate) fn evaluate_held_item(
    evaluator: &Evaluator<'_>,
    rig: &assets::EntityRigBinding,
    variables: &mut MolangVariables,
    budget: &mut EvalBudget<'_>,
) -> Result<Option<f32>, EvalError> {
    rig.held_item_scale
        .map(|expression| {
            let value = evaluator.number(expression as usize, variables, 1.0, budget)?;
            Ok(if value.is_finite() && value > 0.0 {
                value
            } else {
                1.0
            })
        })
        .transpose()
}
