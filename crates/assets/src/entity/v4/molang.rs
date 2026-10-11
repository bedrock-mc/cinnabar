use serde::{Deserialize, Serialize};

use super::super::query_manifest::MOLANG_QUERIES;
use crate::AssetError;

use super::super::{
    CompiledEntityAssets, EntityGeometryScalar, MAX_ENTITY_IDENTIFIER_BYTES, invalid,
    validate_geometry_scalar, validate_identifier,
};
pub use super::molang_math::{MolangEaseCurve, MolangEaseMode, MolangFunction, molang_call};
use super::{
    MAX_MOLANG_COLLECTION_ITEMS, MAX_MOLANG_COLLECTION_ITEMS_TOTAL, MAX_MOLANG_COLLECTIONS,
    MAX_MOLANG_EXPRESSIONS, MAX_MOLANG_OPS, MAX_MOLANG_OPS_PER_EXPRESSION, MAX_MOLANG_STACK_DEPTH,
    MolangSymbol, MolangSymbolKind, range_in_bounds, validate_flattened_ranges,
};

/// Deepest `loop`/`for_each` nesting a program may open.
pub const MAX_MOLANG_LOOP_DEPTH: usize = 8;
/// Most iterations one `loop` runs: the publicly documented limit, kept as a Cinnabar bound
/// although the reference client enforces none, so a long loop cannot freeze an actor.
pub const MAX_MOLANG_LOOP_ITERATIONS: u32 = 1_024;
/// Most arguments one query call may pass.
pub const MAX_MOLANG_QUERY_ARGUMENTS: u8 = 16;

/// One instruction of a compiled Molang program. Jump targets are op offsets inside the same
/// program; the program length addresses its end, where exactly one value must remain.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "op", content = "operand")]
pub enum MolangOp {
    Push(EntityGeometryScalar),
    PushString(u32),
    /// Value earlier animations produced for the channel being evaluated.
    LoadThis,
    LoadQuery(u32),
    CallQuery(MolangCall),
    LoadVariable(u32),
    /// Pops the value into a variable or temporary.
    StoreVariable(u32),
    /// Pushes the variable and jumps when it has been assigned; otherwise falls through to
    /// the fallback operand.
    Coalesce(MolangBranch),
    SelectCollection(u32),
    Pop,
    Negate,
    Not,
    /// Replaces the top value with 1.0 or 0.0 by Molang truthiness.
    Truthy,
    Add,
    Subtract,
    Multiply,
    Divide,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Equal,
    NotEqual,
    Call(MolangFunction),
    Jump(u16),
    JumpIfFalse(u16),
    JumpIfTrue(u16),
    /// Ends the program with the popped value.
    Return,
    /// Pops a count and opens a loop frame, or jumps past the loop when the count is not
    /// positive.
    LoopStart(u16),
    /// Closes one iteration: jumps back to the body while iterations remain.
    LoopNext(u16),
    /// Closes the innermost loop frame and jumps past it.
    LoopBreak(u16),
    /// Pops an actor array; jumps past the loop when it has no elements.
    ForEachStart(MolangBranch),
    ForEachNext(MolangBranch),
    /// Pops an actor reference; anything else pushes 0.0 and jumps past the right side.
    Arrow(u16),
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MolangCall {
    pub symbol: u32,
    pub arguments: u8,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MolangBranch {
    pub symbol: u32,
    pub target: u16,
}

/// Longest string literal a program may push.
pub const MAX_MOLANG_STRING_BYTES: usize = MAX_ENTITY_IDENTIFIER_BYTES;

pub(super) fn validate_molang_payload(compiled: &CompiledEntityAssets) -> Result<(), AssetError> {
    if compiled.molang_symbols.len() > MAX_MOLANG_EXPRESSIONS
        || compiled.molang_expressions.len() > MAX_MOLANG_EXPRESSIONS
        || compiled.molang_ops.len() > MAX_MOLANG_OPS
        || compiled.molang_collections.len() > MAX_MOLANG_COLLECTIONS
        || compiled.molang_collection_items.len() > MAX_MOLANG_COLLECTION_ITEMS_TOTAL
    {
        return Err(invalid("Molang payload count exceeds bound"));
    }
    let mut previous: Option<(MolangSymbolKind, &str)> = None;
    for symbol in &compiled.molang_symbols {
        validate_molang_symbol(symbol)?;
        let key = (symbol.kind, symbol.identifier.as_ref());
        if previous.is_some_and(|value| value >= key) {
            return Err(invalid("Molang symbols are not strictly ordered"));
        }
        previous = Some(key);
    }
    for expression in &compiled.molang_expressions {
        if expression.op_count == 0
            || expression.op_count as usize > MAX_MOLANG_OPS_PER_EXPRESSION
            || expression.max_stack > MAX_MOLANG_STACK_DEPTH
            || !range_in_bounds(
                expression.first_op,
                u32::from(expression.op_count),
                compiled.molang_ops.len(),
            )
        {
            return Err(invalid("invalid Molang expression range or stack bound"));
        }
        let start = expression.first_op as usize;
        let end = start + expression.op_count as usize;
        let ops = &compiled.molang_ops[start..end];
        for op in ops {
            validate_operands(compiled, op)?;
        }
        if molang_program_stack(ops)? != expression.max_stack {
            return Err(invalid("Molang expression declared stack is not exact"));
        }
    }
    validate_flattened_ranges(
        compiled
            .molang_expressions
            .iter()
            .map(|expression| (expression.first_op, u32::from(expression.op_count))),
        compiled.molang_ops.len(),
        "Molang operation",
    )?;
    for collection in &compiled.molang_collections {
        if collection.item_count == 0
            || collection.item_count as usize > MAX_MOLANG_COLLECTION_ITEMS
            || !range_in_bounds(
                collection.first_item,
                u32::from(collection.item_count),
                compiled.molang_collection_items.len(),
            )
        {
            return Err(invalid("invalid Molang collection range"));
        }
    }
    validate_flattened_ranges(
        compiled
            .molang_collections
            .iter()
            .map(|collection| (collection.first_item, u32::from(collection.item_count))),
        compiled.molang_collection_items.len(),
        "Molang collection item",
    )?;
    for item in &compiled.molang_collection_items {
        validate_geometry_scalar(item.value)?;
    }
    Ok(())
}

fn validate_operands(compiled: &CompiledEntityAssets, op: &MolangOp) -> Result<(), AssetError> {
    let kinds: &[MolangSymbolKind] = &[MolangSymbolKind::Variable, MolangSymbolKind::Temporary];
    let valid = match *op {
        MolangOp::Push(value) => {
            validate_geometry_scalar(value)?;
            true
        }
        MolangOp::PushString(symbol) => {
            molang_symbol_has_kind(compiled, symbol, &[MolangSymbolKind::String])
        }
        MolangOp::LoadQuery(symbol) => {
            molang_symbol_has_kind(compiled, symbol, &[MolangSymbolKind::Query])
        }
        MolangOp::CallQuery(call) => {
            call.arguments <= MAX_MOLANG_QUERY_ARGUMENTS
                && molang_symbol_has_kind(compiled, call.symbol, &[MolangSymbolKind::Query])
        }
        MolangOp::LoadVariable(symbol) | MolangOp::StoreVariable(symbol) => {
            molang_symbol_has_kind(compiled, symbol, kinds)
        }
        MolangOp::Coalesce(branch)
        | MolangOp::ForEachStart(branch)
        | MolangOp::ForEachNext(branch) => molang_symbol_has_kind(compiled, branch.symbol, kinds),
        MolangOp::SelectCollection(collection) => {
            (collection as usize) < compiled.molang_collections.len()
        }
        _ => true,
    };
    if valid {
        Ok(())
    } else {
        Err(invalid("Molang operand kind or index is invalid"))
    }
}

/// Proves every path through a program keeps a consistent stack and loop depth, stays in
/// bounds, and ends with one value; returns the deepest stack any path reaches.
pub fn molang_program_stack(ops: &[MolangOp]) -> Result<u8, AssetError> {
    let end = ops.len();
    let mut states: Vec<Option<(usize, usize)>> = vec![None; end + 1];
    let mut pending = vec![(0_usize, (0_usize, 0_usize))];
    let mut deepest = 0_usize;
    let fail = |detail: &'static str| Err(invalid(detail));
    while let Some((at, state)) = pending.pop() {
        match states[at] {
            Some(existing) if existing == state => continue,
            Some(_) => return fail("Molang paths join with different stack depths"),
            None => states[at] = Some(state),
        }
        let (depth, loops) = state;
        deepest = deepest.max(depth);
        if deepest > MAX_MOLANG_STACK_DEPTH as usize || loops > MAX_MOLANG_LOOP_DEPTH {
            return fail("Molang program exceeds its stack or loop bound");
        }
        if at == end {
            if depth != 1 {
                return fail("Molang program must end with exactly one value");
            }
            continue;
        }
        let target = |offset: u16| -> Result<usize, AssetError> {
            let offset = offset as usize;
            if offset > end
                || (offset <= at
                    && !matches!(ops[at], MolangOp::LoopNext(_) | MolangOp::ForEachNext(_)))
            {
                Err(invalid("Molang jump target is out of range"))
            } else {
                Ok(offset)
            }
        };
        let need = |count: usize| -> Result<usize, AssetError> {
            depth
                .checked_sub(count)
                .ok_or_else(|| invalid("Molang expression stack underflows"))
        };
        let next = at + 1;
        match ops[at] {
            MolangOp::Push(_)
            | MolangOp::PushString(_)
            | MolangOp::LoadThis
            | MolangOp::LoadQuery(_)
            | MolangOp::LoadVariable(_) => pending.push((next, (depth + 1, loops))),
            MolangOp::CallQuery(call) => {
                pending.push((next, (need(call.arguments as usize)? + 1, loops)));
            }
            MolangOp::StoreVariable(_) | MolangOp::Pop => pending.push((next, (need(1)?, loops))),
            MolangOp::Coalesce(branch) => {
                pending.push((next, (depth, loops)));
                pending.push((target(branch.target)?, (depth + 1, loops)));
            }
            MolangOp::SelectCollection(_) | MolangOp::Negate | MolangOp::Not | MolangOp::Truthy => {
                pending.push((next, (need(1)? + 1, loops)))
            }
            MolangOp::Add
            | MolangOp::Subtract
            | MolangOp::Multiply
            | MolangOp::Divide
            | MolangOp::Less
            | MolangOp::LessEqual
            | MolangOp::Greater
            | MolangOp::GreaterEqual
            | MolangOp::Equal
            | MolangOp::NotEqual => pending.push((next, (need(2)? + 1, loops))),
            MolangOp::Call(function) => {
                pending.push((next, (need(function.arity())? + 1, loops)));
            }
            MolangOp::Jump(offset) => pending.push((target(offset)?, (depth, loops))),
            MolangOp::JumpIfFalse(offset) | MolangOp::JumpIfTrue(offset) => {
                let popped = need(1)?;
                pending.push((next, (popped, loops)));
                pending.push((target(offset)?, (popped, loops)));
            }
            MolangOp::Return => {
                need(1)?;
            }
            MolangOp::LoopStart(offset) => {
                let popped = need(1)?;
                pending.push((next, (popped, loops + 1)));
                pending.push((target(offset)?, (popped, loops)));
            }
            MolangOp::ForEachStart(branch) => {
                let popped = need(1)?;
                pending.push((next, (popped, loops + 1)));
                pending.push((target(branch.target)?, (popped, loops)));
            }
            MolangOp::LoopNext(offset)
            | MolangOp::ForEachNext(MolangBranch { target: offset, .. }) => {
                let closed = loops
                    .checked_sub(1)
                    .ok_or_else(|| invalid("Molang loop step is outside a loop"))?;
                pending.push((target(offset)?, (depth, loops)));
                pending.push((next, (depth, closed)));
            }
            MolangOp::LoopBreak(offset) => {
                let closed = loops
                    .checked_sub(1)
                    .ok_or_else(|| invalid("Molang break is outside a loop"))?;
                pending.push((target(offset)?, (depth, closed)));
            }
            MolangOp::Arrow(offset) => {
                let popped = need(1)?;
                pending.push((next, (popped, loops)));
                pending.push((target(offset)?, (popped + 1, loops)));
            }
        }
    }
    u8::try_from(deepest).map_err(|_| invalid("Molang program exceeds its stack bound"))
}

fn validate_molang_symbol(symbol: &MolangSymbol) -> Result<(), AssetError> {
    let valid = match symbol.kind {
        MolangSymbolKind::String => {
            symbol.identifier.len() <= MAX_MOLANG_STRING_BYTES
                && !symbol.identifier.chars().any(char::is_control)
        }
        kind => {
            validate_identifier(&symbol.identifier)?;
            match kind {
                MolangSymbolKind::Name => !["query.", "variable.", "temp.", "context."]
                    .iter()
                    .any(|prefix| symbol.identifier.starts_with(prefix)),
                MolangSymbolKind::Query => MOLANG_QUERIES
                    .binary_search(&symbol.identifier.as_ref())
                    .is_ok(),
                MolangSymbolKind::Variable => {
                    valid_molang_slot(&symbol.identifier, "variable.")
                        || valid_molang_slot(&symbol.identifier, "context.")
                }
                MolangSymbolKind::Temporary => valid_molang_slot(&symbol.identifier, "temp."),
                MolangSymbolKind::String => unreachable!(),
            }
        }
    };
    if !valid {
        return Err(invalid("Molang symbol is outside the reviewed namespace"));
    }
    Ok(())
}

/// A variable path: dot-separated lowercase segments after the namespace prefix.
fn valid_molang_slot(identifier: &str, prefix: &str) -> bool {
    identifier.strip_prefix(prefix).is_some_and(|path| {
        path.split('.').all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        })
    })
}

pub(super) fn molang_symbol_has_kind(
    compiled: &CompiledEntityAssets,
    index: u32,
    permitted: &[MolangSymbolKind],
) -> bool {
    compiled
        .molang_symbols
        .get(index as usize)
        .is_some_and(|symbol| permitted.contains(&symbol.kind))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reviewed_query_namespace_is_sorted_and_unique() {
        assert!(MOLANG_QUERIES.windows(2).all(|pair| pair[0] < pair[1]));
    }
}
