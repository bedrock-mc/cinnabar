//! Expression results and effects retained by a rig's isolated frame scratch.
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

#[derive(Debug)]
struct Occurrences {
    entries: Vec<Expression>,
    cursor: usize,
    last_used: Option<usize>,
    direct_read: bool,
}

impl Expression {
    /// Keeps a separate result and effect journal for one invocation within a frame.
    fn new(pure: Inputs) -> Self {
        Self {
            pure,
            observed: None,
            this: 0,
            value: None,
            effects: MolangEffects::default(),
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct Expressions(BTreeMap<usize, Occurrences>);

impl Expressions {
    /// Rewinds invocation positions while retaining each occurrence's buffers and results.
    pub(crate) fn begin(&mut self) {
        for occurrences in self.0.values_mut() {
            occurrences.cursor = 0;
            occurrences.last_used = None;
        }
    }

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
        let occurrences = self.0.entry(index).or_insert_with(|| Occurrences {
            direct_read: evaluator
                .assets
                .molang_expressions()
                .get(index)
                .is_some_and(|expression| {
                    expression.op_count == 1
                        && match evaluator
                            .assets
                            .molang_ops()
                            .get(expression.first_op as usize)
                        {
                            Some(MolangOp::LoadQuery(_)) => true,
                            Some(MolangOp::LoadVariable(symbol)) => {
                                evaluator.layout.place(*symbol).is_some()
                            }
                            Some(MolangOp::CallQuery(call)) => call.arguments == 0,
                            _ => false,
                        }
                }),
            entries: vec![Expression::new(Inputs::new(
                evaluator.assets,
                [index as u32],
            ))],
            cursor: 0,
            last_used: None,
        });
        let slot = occurrences.cursor;
        occurrences.cursor += 1;
        if slot == occurrences.entries.len() {
            let inputs = occurrences.entries[0].pure.fresh();
            occurrences.entries.push(Expression::new(inputs));
        }
        let (earlier, remaining) = occurrences.entries.split_at_mut(slot);
        let cached = &mut remaining[0];
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
            occurrences.last_used = Some(slot);
            return Ok(value.clone());
        }
        // A shared program often supplies several identical channels. Try only the last
        // successful occurrence, so validating repeated programs stays linear in their uses.
        if let Some(previous) = occurrences.last_used.and_then(|slot| earlier.get(slot))
            && previous.this == this.to_bits()
            && let Some(value) = &previous.value
        {
            if cached.pure.pure() && cached.pure.same_values(&previous.pure) {
                cached.this = this.to_bits();
                cached.value = Some(value.clone());
                occurrences.last_used = Some(slot);
                return Ok(value.clone());
            }
            if !cached.pure.pure()
                && let Some(inputs) = &previous.observed
            {
                let mut validation = budget.work_left;
                if inputs
                    .matches(evaluator, variables, &mut validation)
                    .unwrap_or(false)
                {
                    previous.effects.apply(variables)?;
                    return Ok(value.clone());
                }
            }
        }
        cached.value = None;
        cached.this = this.to_bits();
        if occurrences.direct_read
            && let Some(value) = cached.pure.single_value()
        {
            budget.charge()?;
            #[cfg(test)]
            super::record_expression_work(index);
            cached.value = Some(value.clone());
            occurrences.last_used = Some(slot);
            return Ok(value);
        }
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
        occurrences.last_used = Some(slot);
        Ok(value)
    }
}
