//! Exact dependencies for sampled work in one rig's immutable asset layout.
use super::*;

#[derive(Debug, PartialEq)]
enum Value {
    Unset,
    Number(u32),
    String(Arc<str>),
    Actor(u64),
}

impl Value {
    /// Preserves type, assignment presence and floating-point bits in cache keys.
    fn new(value: Option<evaluation::MolangValue>) -> Self {
        match value {
            None => Self::Unset,
            Some(evaluation::MolangValue::Number(value)) => Self::Number(value.to_bits()),
            Some(evaluation::MolangValue::String(value)) => Self::String(value),
            Some(evaluation::MolangValue::ActorReference(value)) => Self::Actor(value),
        }
    }
}

/// Expressions with writes, randomness or dynamic query arguments keep the ordinary path.
#[derive(Debug)]
pub(crate) struct Inputs {
    symbols: Arc<[u32]>,
    values: Vec<Value>,
    pure: bool,
    valid: bool,
}

impl Inputs {
    /// Derives dependency identities from authored bytecode, without a maintained query list.
    pub(crate) fn new(
        assets: &RuntimeEntityAssets,
        expressions: impl IntoIterator<Item = u32>,
    ) -> Self {
        Self::build(assets, expressions, false)
    }

    /// Whole-frame inputs retain tick-owned variables and validate every external query.
    pub(crate) fn queries(
        assets: &RuntimeEntityAssets,
        expressions: impl IntoIterator<Item = u32>,
    ) -> Self {
        Self::build(assets, expressions, true)
    }

    /// Frame keys already own variable, clock, controller and random-stream identities.
    fn build(
        assets: &RuntimeEntityAssets,
        expressions: impl IntoIterator<Item = u32>,
        frame: bool,
    ) -> Self {
        let mut symbols = std::collections::BTreeSet::new();
        let mut pure = true;
        for index in expressions {
            let Some(expression) = assets.molang_expressions().get(index as usize) else {
                pure = false;
                continue;
            };
            let first = expression.first_op as usize;
            let Some(ops) = assets
                .molang_ops()
                .get(first..first + usize::from(expression.op_count))
            else {
                pure = false;
                continue;
            };
            for op in ops {
                match op {
                    MolangOp::LoadQuery(symbol) => {
                        symbols.insert(*symbol);
                    }
                    MolangOp::CallQuery(call) if call.arguments == 0 => {
                        symbols.insert(call.symbol);
                    }
                    MolangOp::LoadVariable(symbol) if !frame => {
                        symbols.insert(*symbol);
                    }
                    MolangOp::Call(function) if !frame && function.is_random() => pure = false,
                    MolangOp::CallQuery(_) | MolangOp::Arrow(_) => pure = false,
                    MolangOp::StoreVariable(_)
                    | MolangOp::Coalesce(_)
                    | MolangOp::ForEachStart(_)
                    | MolangOp::ForEachNext(_)
                        if !frame =>
                    {
                        pure = false
                    }
                    _ => {}
                }
            }
        }
        pure &= symbols
            .iter()
            .all(|&symbol| assets.molang_symbols().get(symbol as usize).is_some());
        Self {
            symbols: symbols.into_iter().collect::<Vec<_>>().into(),
            values: Vec::new(),
            pure,
            valid: false,
        }
    }

    /// A single-read program can use its validated input without evaluating the query twice.
    pub(crate) fn single_value(&self) -> Option<evaluation::MolangValue> {
        if !self.valid {
            return None;
        }
        let [value] = self.values.as_slice() else {
            return None;
        };
        Some(match value {
            Value::Unset => evaluation::MolangValue::Number(0.0),
            Value::Number(bits) => evaluation::MolangValue::Number(f32::from_bits(*bits)),
            Value::String(value) => evaluation::MolangValue::String(Arc::clone(value)),
            Value::Actor(value) => evaluation::MolangValue::ActorReference(*value),
        })
    }

    /// Shares immutable query identities across actors and completed ticks.
    pub(crate) fn query_plan(&self) -> Option<Arc<[u32]>> {
        self.pure.then(|| Arc::clone(&self.symbols))
    }

    /// Creates empty sample values for an already analyzed immutable query plan.
    pub(crate) fn from_query_plan(plan: Option<Arc<[u32]>>) -> Self {
        let pure = plan.is_some();
        Self {
            symbols: plan.unwrap_or_else(|| Arc::from([])),
            values: Vec::new(),
            pure,
            valid: false,
        }
    }

    /// Changes the tick's query topology without discarding its sample-value allocation.
    pub(crate) fn reset_query_plan(&mut self, plan: Option<Arc<[u32]>>) {
        self.pure = plan.is_some();
        if let Some(symbols) = plan {
            self.symbols = symbols;
        }
        self.values.clear();
        self.valid = false;
    }

    /// Whether the bytecode can be sampled without authored writes or random draws.
    pub(crate) fn pure(&self) -> bool {
        self.pure
    }

    /// Shares a program's read identities while starting an independent occurrence key.
    pub(crate) fn fresh(&self) -> Self {
        Self {
            symbols: Arc::clone(&self.symbols),
            values: Vec::new(),
            pure: self.pure,
            valid: false,
        }
    }

    /// Compares validated occurrences of the same immutable program without querying twice.
    pub(crate) fn same_values(&self, other: &Self) -> bool {
        self.valid
            && other.valid
            && Arc::ptr_eq(&self.symbols, &other.symbols)
            && self.values == other.values
    }

    /// Compares only actual reads; changing a frame fraction affects only its readers.
    pub(crate) fn update(
        &mut self,
        evaluator: &Evaluator<'_>,
        variables: &MolangVariables,
        budget: &EvalBudget<'_>,
    ) -> bool {
        if !self.pure || self.symbols.len() > budget.work_left {
            self.valid = false;
            return false;
        }
        let mut unchanged =
            std::mem::replace(&mut self.valid, false) && self.values.len() == self.symbols.len();
        for (index, &symbol) in self.symbols.iter().enumerate() {
            let value = Value::new(
                match evaluator.assets.molang_symbols()[symbol as usize].kind {
                    assets::MolangSymbolKind::Query => Some(evaluator.query(symbol, &[])),
                    _ => evaluator.layout.input_variable(variables, symbol),
                },
            );
            if let Some(previous) = self.values.get_mut(index) {
                unchanged &= *previous == value;
                *previous = value;
            } else {
                self.values.push(value);
            }
        }
        self.valid = true;
        unchanged
    }
}

#[cfg(test)]
thread_local! { static ENABLED: std::cell::Cell<bool> = const { std::cell::Cell::new(true) }; }

/// Production samples always cache; regression tests can run the same path without reuse.
pub(crate) fn enabled() -> bool {
    #[cfg(test)]
    {
        ENABLED.with(|enabled| enabled.get())
    }
    #[cfg(not(test))]
    {
        true
    }
}

/// Runs a behavior control while restoring the calling thread's cache setting on every exit.
#[cfg(test)]
pub(super) fn uncached<T>(sample: impl FnOnce() -> T) -> T {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            ENABLED.with(|enabled| enabled.set(self.0));
        }
    }
    let _restore = Restore(ENABLED.with(|enabled| enabled.replace(false)));
    sample()
}
