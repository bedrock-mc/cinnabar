use std::collections::{BTreeMap, BTreeSet, HashMap};

use assets::{
    AssetError, CompiledMolangExpression, EntityGeometryScalar, MAX_MOLANG_EXPRESSIONS,
    MAX_MOLANG_OPS, MAX_MOLANG_OPS_PER_EXPRESSION, MolangBranch, MolangCall, MolangOp,
    MolangProgram, MolangSymbol, MolangSymbolKind, molang_call, molang_program_stack,
};

use super::invalid;

mod codegen;
mod lexer;
mod parser;

use codegen::{Codegen, IrOp, binary, fold_with};
use parser::{Binary, Expr, Program, Slot, Unary, parse, parse_with};

/// Both block-state query spellings resolve the same named state.
const BLOCK_QUERIES: &[&str] = &["query.block_property", "query.block_state"];
/// Longest block expression accepted; bounds the depth of its tree, which is walked recursively.
const MAX_BLOCK_MOLANG_BYTES: usize = 2048;

/// A block-state query result; integer and boolean states read as numbers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BlockStateValue<'a> {
    Number(f32),
    String(&'a str),
}

/// A block permutation condition or bone visibility expression, parsed once for every state.
#[derive(Debug)]
pub struct BlockMolang(Expr);

impl BlockMolang {
    /// `None` when the source does not parse in a block context, which reads only
    /// named block states.
    #[must_use]
    pub fn parse(source: &str) -> Option<Self> {
        if source.len() > MAX_BLOCK_MOLANG_BYTES {
            return None;
        }
        match parse_with(source, BLOCK_QUERIES).ok()? {
            Program::Simple(expression) => Some(Self(expression)),
            Program::Complex(_) => None,
        }
    }

    /// The value for one block state; `None` when it reads a state the block lacks or uses a
    /// string where a number is needed.
    #[must_use]
    pub fn evaluate<'a>(
        &self,
        block_state: &dyn Fn(&str) -> Option<BlockStateValue<'a>>,
    ) -> Option<f32> {
        block_value(&self.0, block_state)?.number()
    }
}

impl BlockStateValue<'_> {
    fn number(self) -> Option<f32> {
        match self {
            Self::Number(value) => Some(value),
            Self::String(_) => None,
        }
    }
}

fn block_value<'e, 'a: 'e>(
    expression: &'e Expr,
    block_state: &dyn Fn(&str) -> Option<BlockStateValue<'a>>,
) -> Option<BlockStateValue<'e>> {
    let number = |expression: &'e Expr| block_value(expression, block_state)?.number();
    let truth = |value: bool| BlockStateValue::Number(if value { 1.0 } else { 0.0 });
    Some(match expression {
        Expr::Number(value) => BlockStateValue::Number(*value),
        Expr::String(text) => BlockStateValue::String(text),
        // A block context has no variable storage, so every variable reads as unset.
        Expr::Variable(..) => BlockStateValue::Number(0.0),
        Expr::Query(_, Some(arguments)) => match arguments.as_slice() {
            [Expr::String(name)] => block_state(name)?,
            _ => return None,
        },
        Expr::Unary(Unary::Negate, value) => BlockStateValue::Number(-number(value)?),
        Expr::Unary(Unary::Not, value) => truth(number(value)? == 0.0),
        Expr::Binary(operator @ (Binary::Equal | Binary::NotEqual), left, right) => {
            let equal = match (
                block_value(left, block_state)?,
                block_value(right, block_state)?,
            ) {
                (BlockStateValue::Number(left), BlockStateValue::Number(right)) => left == right,
                (BlockStateValue::String(left), BlockStateValue::String(right)) => left == right,
                _ => return None,
            };
            truth(equal == (*operator == Binary::Equal))
        }
        Expr::Binary(operator, left, right) => {
            BlockStateValue::Number(binary(*operator, number(left)?, number(right)?))
        }
        Expr::And(left, right) => truth(number(left)? != 0.0 && number(right)? != 0.0),
        Expr::Or(left, right) => truth(number(left)? != 0.0 || number(right)? != 0.0),
        Expr::Conditional(condition, yes, no) => {
            if number(condition)? != 0.0 {
                block_value(yes, block_state)?
            } else {
                match no {
                    Some(no) => block_value(no, block_state)?,
                    None => BlockStateValue::Number(0.0),
                }
            }
        }
        Expr::Call(function, arguments) if !function.is_random() => {
            let values = arguments.iter().map(number).collect::<Option<Vec<_>>>()?;
            BlockStateValue::Number(molang_call(*function, &values, &mut || 0.0))
        }
        _ => return None,
    })
}

#[derive(Clone, Default)]
pub(super) struct MolangCompiler {
    programs: Vec<Vec<IrOp>>,
    interned: HashMap<Box<str>, u32>,
    names: BTreeSet<Box<str>>,
}

/// Compiler length before a speculative compile, restored when it fails.
#[derive(Clone, Copy)]
pub(super) struct MolangMark(usize);

impl MolangCompiler {
    /// Compiles one expression. An error means vanilla would evaluate it as 0.0.
    pub fn compile(&mut self, source: &str) -> Result<u32, AssetError> {
        if let Some(&index) = self.interned.get(source) {
            return Ok(index);
        }
        let program = Codegen::program(&parse(source)?)?;
        let index = self.push(program)?;
        self.interned.insert(source.into(), index);
        Ok(index)
    }

    /// Compiles script entries as the one program their concatenation forms, since vanilla
    /// packs split single statements across entries; returns it and the number of entries
    /// lost, which is all of them when the program does not compile.
    pub fn compile_script(&mut self, sources: &[&str]) -> Result<(Option<u32>, usize), AssetError> {
        if sources.is_empty() {
            return Ok((None, 0));
        }
        let joined = sources.join("\n");
        match parse(&joined)
            .and_then(|program| Codegen::program(&program))
            .and_then(|program| self.push(program))
        {
            Ok(index) => Ok((Some(index), 0)),
            Err(_) => Ok((None, sources.len())),
        }
    }

    /// Compiles an authored script field: one string or an array of strings. Any other shape
    /// cannot compile, so the whole script is dropped and counted, like uncompilable text.
    pub fn compile_script_value(
        &mut self,
        value: Option<&serde_json::Value>,
    ) -> Result<(Option<u32>, usize), AssetError> {
        match value {
            None => Ok((None, 0)),
            Some(serde_json::Value::String(statement)) => self.compile_script(&[statement]),
            Some(serde_json::Value::Array(entries)) => {
                match entries
                    .iter()
                    .map(serde_json::Value::as_str)
                    .collect::<Option<Vec<_>>>()
                {
                    Some(statements) => self.compile_script(&statements),
                    None => Ok((None, entries.len().max(1))),
                }
            }
            Some(_) => Ok((None, 1)),
        }
    }

    /// Evaluates a constant reading with every query and variable at zero.
    pub fn evaluate_default(source: &str) -> Option<f32> {
        match parse(source).ok()? {
            Program::Simple(expression) => fold_with(&expression, &|read| match read {
                Expr::Variable(..) | Expr::Query(..) => Some(0.0),
                _ => None,
            }),
            Program::Complex(_) => None,
        }
    }

    pub fn add_name(&mut self, name: &str) -> Result<(), AssetError> {
        if name.is_empty() {
            return Err(invalid("empty Molang name"));
        }
        self.names.insert(name.into());
        Ok(())
    }

    pub fn mark(&self) -> MolangMark {
        MolangMark(self.programs.len())
    }

    pub fn rollback(&mut self, mark: MolangMark) {
        self.programs.truncate(mark.0);
        self.interned.retain(|_, index| (*index as usize) < mark.0);
    }

    fn push(&mut self, program: Vec<IrOp>) -> Result<u32, AssetError> {
        if self.programs.len() >= MAX_MOLANG_EXPRESSIONS {
            return Err(invalid("Molang expression count exceeds bound"));
        }
        if program.is_empty() || program.len() > MAX_MOLANG_OPS_PER_EXPRESSION {
            return Err(invalid("Molang operation count exceeds bound"));
        }
        self.programs.push(program);
        Ok(self.programs.len() as u32 - 1)
    }

    pub fn finish(self) -> Result<MolangProgram, AssetError> {
        let mut symbol_set = self
            .names
            .into_iter()
            .map(|name| (MolangSymbolKind::Name, name))
            .collect::<BTreeSet<_>>();
        for op in self.programs.iter().flatten() {
            if let Some(symbol) = symbol_of(op) {
                symbol_set.insert(symbol);
            }
        }
        let symbols = symbol_set
            .into_iter()
            .map(|(kind, identifier)| MolangSymbol { kind, identifier })
            .collect::<Vec<_>>();
        let indices = symbols
            .iter()
            .enumerate()
            .map(|(index, symbol)| ((symbol.kind, symbol.identifier.clone()), index as u32))
            .collect::<BTreeMap<_, _>>();
        let mut ops = Vec::new();
        let mut expressions = Vec::with_capacity(self.programs.len());
        for program in self.programs {
            let first_op = ops.len();
            for op in program {
                ops.push(lower(op, &indices)?);
            }
            let max_stack = molang_program_stack(&ops[first_op..])?;
            expressions.push(CompiledMolangExpression {
                first_op: first_op as u32,
                op_count: (ops.len() - first_op) as u16,
                max_stack,
            });
        }
        if ops.len() > MAX_MOLANG_OPS {
            return Err(invalid("total Molang operation count exceeds bound"));
        }
        Ok(MolangProgram::new(
            symbols.into_boxed_slice(),
            expressions.into_boxed_slice(),
            ops.into_boxed_slice(),
            Box::new([]),
            Box::new([]),
        ))
    }
}

fn slot_kind(slot: Slot) -> MolangSymbolKind {
    match slot {
        Slot::Variable => MolangSymbolKind::Variable,
        Slot::Temporary => MolangSymbolKind::Temporary,
    }
}

fn symbol_of(op: &IrOp) -> Option<(MolangSymbolKind, Box<str>)> {
    Some(match op {
        IrOp::PushString(text) => (MolangSymbolKind::String, text.clone()),
        IrOp::LoadQuery(name) | IrOp::CallQuery(name, _) => (MolangSymbolKind::Query, name.clone()),
        IrOp::LoadVariable(slot, name)
        | IrOp::StoreVariable(slot, name)
        | IrOp::Coalesce(slot, name, _)
        | IrOp::ForEachStart(slot, name, _)
        | IrOp::ForEachNext(slot, name, _) => (slot_kind(*slot), name.clone()),
        _ => return None,
    })
}

fn lower(
    op: IrOp,
    indices: &BTreeMap<(MolangSymbolKind, Box<str>), u32>,
) -> Result<MolangOp, AssetError> {
    let index = |kind: MolangSymbolKind, name: &str| {
        indices
            .get(&(kind, name.into()))
            .copied()
            .ok_or_else(|| invalid("Molang symbol was not interned"))
    };
    let target = |offset: usize| {
        u16::try_from(offset).map_err(|_| invalid("Molang jump target exceeds bound"))
    };
    Ok(match op {
        IrOp::Push(value) => MolangOp::Push(
            EntityGeometryScalar::new(value)
                .ok_or_else(|| invalid("non-finite Molang constant"))?,
        ),
        IrOp::PushString(text) => MolangOp::PushString(index(MolangSymbolKind::String, &text)?),
        IrOp::LoadThis => MolangOp::LoadThis,
        IrOp::LoadQuery(name) => MolangOp::LoadQuery(index(MolangSymbolKind::Query, &name)?),
        IrOp::CallQuery(name, arguments) => MolangOp::CallQuery(MolangCall {
            symbol: index(MolangSymbolKind::Query, &name)?,
            arguments,
        }),
        IrOp::LoadVariable(slot, name) => MolangOp::LoadVariable(index(slot_kind(slot), &name)?),
        IrOp::StoreVariable(slot, name) => MolangOp::StoreVariable(index(slot_kind(slot), &name)?),
        IrOp::Coalesce(slot, name, label) => MolangOp::Coalesce(MolangBranch {
            symbol: index(slot_kind(slot), &name)?,
            target: target(label.offset())?,
        }),
        IrOp::Pop => MolangOp::Pop,
        IrOp::Negate => MolangOp::Negate,
        IrOp::Not => MolangOp::Not,
        IrOp::Truthy => MolangOp::Truthy,
        IrOp::Binary(operator) => match operator {
            Binary::Add => MolangOp::Add,
            Binary::Subtract => MolangOp::Subtract,
            Binary::Multiply => MolangOp::Multiply,
            Binary::Divide => MolangOp::Divide,
            Binary::Less => MolangOp::Less,
            Binary::LessEqual => MolangOp::LessEqual,
            Binary::Greater => MolangOp::Greater,
            Binary::GreaterEqual => MolangOp::GreaterEqual,
            Binary::Equal => MolangOp::Equal,
            Binary::NotEqual => MolangOp::NotEqual,
        },
        IrOp::Call(function) => MolangOp::Call(function),
        IrOp::Jump(label) => MolangOp::Jump(target(label.offset())?),
        IrOp::JumpIfFalse(label) => MolangOp::JumpIfFalse(target(label.offset())?),
        IrOp::JumpIfTrue(label) => MolangOp::JumpIfTrue(target(label.offset())?),
        IrOp::Return => MolangOp::Return,
        IrOp::LoopStart(label) => MolangOp::LoopStart(target(label.offset())?),
        IrOp::LoopNext(label) => MolangOp::LoopNext(target(label.offset())?),
        IrOp::LoopBreak(label) => MolangOp::LoopBreak(target(label.offset())?),
        IrOp::ForEachStart(slot, name, label) => MolangOp::ForEachStart(MolangBranch {
            symbol: index(slot_kind(slot), &name)?,
            target: target(label.offset())?,
        }),
        IrOp::ForEachNext(slot, name, label) => MolangOp::ForEachNext(MolangBranch {
            symbol: index(slot_kind(slot), &name)?,
            target: target(label.offset())?,
        }),
        IrOp::Arrow(label) => MolangOp::Arrow(target(label.offset())?),
    })
}

#[cfg(test)]
mod tests;

/// Compiles a server expression with the same bounded parser used for pack scripts.
pub fn compile_molang_expression(source: &str) -> Result<MolangProgram, AssetError> {
    let mut compiler = MolangCompiler::default();
    compiler.compile(source)?;
    compiler.finish()
}

#[cfg(test)]
mod query_inventory_tests {
    use super::*;
    use std::collections::BTreeSet;

    /// Collects query names from independently downloaded pack documents.
    fn pack_queries(root: &Path, names: &mut BTreeSet<String>) {
        for entry in std::fs::read_dir(root).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                pack_queries(&path, names);
            } else if path
                .extension()
                .is_some_and(|extension| extension == "json")
            {
                let text = std::fs::read_to_string(path).unwrap();
                for (index, _) in text.match_indices('.') {
                    let prefix = &text[..index];
                    let before = prefix
                        .strip_suffix("query")
                        .or_else(|| prefix.strip_suffix('q'));
                    if before.is_none_or(|before| {
                        before
                            .as_bytes()
                            .last()
                            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
                    }) {
                        continue;
                    }
                    let length = text[index + 1..]
                        .bytes()
                        .take_while(|byte| {
                            byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_'
                        })
                        .count();
                    if length > 0 {
                        names.insert(format!("query.{}", &text[index + 1..index + 1 + length]));
                    }
                }
            }
        }
    }

    #[test]
    fn downloaded_pack_query_inventory_has_explicit_contracts() {
        let Some(root) = std::env::var_os("CINNABAR_VANILLA_RESOURCE_PACK") else {
            eprintln!("missing fixture: CINNABAR_VANILLA_RESOURCE_PACK pinned resource pack");
            return;
        };
        let root = Path::new(&root);
        let mut inventory = BTreeSet::new();
        for family in [
            "entity",
            "animations",
            "animation_controllers",
            "render_controllers",
            "attachables",
        ] {
            pack_queries(&root.join(family), &mut inventory);
        }
        assert!(!inventory.is_empty(), "pack query inventory is empty");
        for name in inventory {
            let contract = assets::MolangQuery::from_name(&name)
                .unwrap_or_else(|| panic!("query has no supported or unresolved contract: {name}"));
            let program = compile_molang_expression(&name).unwrap();
            let symbol = program
                .symbols
                .iter()
                .position(|symbol| symbol.identifier.as_ref() == name)
                .unwrap();
            assert_eq!(program.query_binding(symbol as u32), Some(contract));
        }
    }
}
