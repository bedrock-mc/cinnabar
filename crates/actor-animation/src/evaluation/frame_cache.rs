//! Expression results and effects retained only in immutable endpoint scratch storage.
use super::*;
use crate::render_frame::cache::Inputs;

#[derive(Debug)]
struct Expression {
    pure: Inputs,
    observed: Option<ObservedInputs>,
    this: u32,
    value: Option<MolangValue>,
    effects: MolangEffects,
}

#[derive(Debug, Default)]
pub(crate) struct Expressions(BTreeMap<usize, Expression>);

impl Expressions {
    /// Validates executed reads and replays writes and random draws on a cache hit.
    pub(crate) fn run(
        &mut self,
        evaluator: &Evaluator<'_>,
        index: usize,
        variables: &mut MolangVariables,
        this: f32,
        budget: &mut EvalBudget<'_>,
    ) -> Result<MolangValue, EvalError> {
        variables.clear_temporaries();
        let cached = self.0.entry(index).or_insert_with(|| Expression {
            pure: Inputs::new(evaluator.assets, [index as u32]),
            observed: None,
            this: this.to_bits(),
            value: None,
            effects: MolangEffects::default(),
        });
        let unchanged = if cached.pure.pure() {
            cached.pure.update(evaluator, variables, budget)
        } else if let Some(inputs) = &cached.observed {
            let mut validation = budget.work_left;
            inputs
                .matches(evaluator, variables, &mut validation)
                .unwrap_or(false)
        } else {
            false
        };
        if unchanged
            && cached.this == this.to_bits()
            && let Some(value) = &cached.value
        {
            cached.effects.apply(variables)?;
            return Ok(value.clone());
        }
        cached.value = None;
        cached.this = this.to_bits();
        let result = if cached.pure.pure() {
            evaluator.run_observed(index, variables, this, budget, None)
        } else {
            let mut inputs = ObservedInputs::new(variables);
            let capture = variables.begin_effects();
            let result = evaluator.run_observed(index, variables, this, budget, Some(&mut inputs));
            cached.effects = variables.finish_effects(capture);
            cached.observed = Some(inputs);
            result
        };
        let value = result?;
        cached.value = Some(value.clone());
        Ok(value)
    }
}
