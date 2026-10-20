//! External reads needed to reuse a pure scratch evaluation without running its bytecode.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug)]
struct QueryRead {
    symbol: u32,
    arguments: Box<[MolangValue]>,
    value: MolangValue,
}

/// The executed branch's input reads, excluding values the expression wrote itself.
#[derive(Debug)]
pub(crate) struct ObservedInputs {
    variables: BTreeMap<u32, Option<MolangValue>>,
    written: BTreeSet<u32>,
    undo: Vec<(Place, Option<MolangValue>)>,
    temporary_checkpoint: usize,
    queries: Vec<QueryRead>,
    random: u64,
}

impl ObservedInputs {
    /// Retains the initial random seed because scratch expressions reset it for each owner.
    pub(crate) fn new(variables: &MolangVariables) -> Self {
        Self {
            variables: BTreeMap::new(),
            written: BTreeSet::new(),
            undo: Vec::new(),
            temporary_checkpoint: variables.temporary_checkpoint(),
            queries: Vec::new(),
            random: variables.random_state(),
        }
    }

    /// Charges each validated input and query argument; exhaustion leaves the result unresolved.
    pub(crate) fn matches(
        &self,
        evaluator: &Evaluator<'_>,
        variables: &MolangVariables,
        operations_left: &mut usize,
    ) -> Option<bool> {
        charge(operations_left, 1)?;
        if self.random != variables.random_state() {
            return Some(false);
        }
        for (&symbol, expected) in &self.variables {
            charge(operations_left, 1)?;
            let Some(Place::Variable(slot)) = evaluator.layout.place(symbol) else {
                return Some(false);
            };
            if !same_value(variables.variable(slot).as_ref(), expected.as_ref()) {
                return Some(false);
            }
        }
        for read in &self.queries {
            charge(operations_left, 1 + read.arguments.len())?;
            if !same_value(
                Some(&evaluator.query(read.symbol, &read.arguments)),
                Some(&read.value),
            ) {
                return Some(false);
            }
        }
        Some(true)
    }

    /// Counts stored values so the owner-selection cache has a shared memory bound.
    pub(crate) fn weight(&self) -> usize {
        1 + self.variables.len()
            + self.written.len()
            + self
                .queries
                .iter()
                .map(|read| 1 + read.arguments.len())
                .sum::<usize>()
    }

    /// Captures only inherited persistent slots read before this expression assigns them.
    pub(super) fn variable(&mut self, symbol: u32, place: Place, variables: &MolangVariables) {
        if let Place::Variable(slot) = place
            && !self.written.contains(&symbol)
        {
            self.variables
                .entry(symbol)
                .or_insert_with(|| variables.variable(slot));
        }
    }

    /// Records each slot's first pre-write value and excludes later reads from inherited inputs.
    pub(super) fn written(&mut self, symbol: u32, place: Place, variables: &MolangVariables) {
        if self.written.insert(symbol) {
            #[cfg(test)]
            super::selection_work::record(|work| work.saved += 1);
            self.undo.push((place, variables.read(place)));
        }
    }

    /// Restores the selection prefix after success or failure, including clean temps and RNG.
    pub(crate) fn restore(&mut self, variables: &mut MolangVariables) {
        variables.restore(std::mem::take(&mut self.undo), self.temporary_checkpoint);
        variables.restore_random(self.random);
    }

    /// Records the exact arguments and value of one query along the executed branch.
    pub(super) fn query(&mut self, symbol: u32, arguments: &[MolangValue], value: &MolangValue) {
        self.queries.push(QueryRead {
            symbol,
            arguments: arguments.into(),
            value: value.clone(),
        });
    }
}

/// Reserves validation work before it runs, consuming the remainder on exhaustion.
fn charge(operations_left: &mut usize, work: usize) -> Option<()> {
    let remaining = operations_left.checked_sub(work);
    *operations_left = remaining.unwrap_or(0);
    remaining.map(|_| ())
}

/// Exact input identity distinguishes signed zero and retains stable NaN payloads.
fn same_value(left: Option<&MolangValue>, right: Option<&MolangValue>) -> bool {
    match (left, right) {
        (Some(MolangValue::Number(left)), Some(MolangValue::Number(right))) => {
            left.to_bits() == right.to_bits()
        }
        _ => left == right,
    }
}
