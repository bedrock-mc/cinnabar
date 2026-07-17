use std::collections::{BTreeMap, BTreeSet};

use assets::{
    AssetError, CompiledMolangExpression, EntityGeometryScalar, MAX_MOLANG_EXPRESSIONS,
    MAX_MOLANG_OPS, MAX_MOLANG_OPS_PER_EXPRESSION, MAX_MOLANG_STACK_DEPTH, MolangCollection,
    MolangCollectionItem, MolangOp, MolangSymbol, MolangSymbolKind,
};

use super::invalid;

const MAX_EXPRESSION_BYTES: usize = 16 * 1024;
const MAX_PARSE_DEPTH: usize = 32;

#[derive(Clone, Default)]
pub(super) struct MolangCompiler {
    expressions: Vec<Expr>,
    names: BTreeSet<Box<str>>,
}

pub(super) struct MolangPayload {
    pub symbols: Box<[MolangSymbol]>,
    pub expressions: Box<[CompiledMolangExpression]>,
    pub ops: Box<[MolangOp]>,
    pub collections: Box<[MolangCollection]>,
    pub collection_items: Box<[MolangCollectionItem]>,
}

impl MolangCompiler {
    pub fn compile(&mut self, source: &str) -> Result<u32, AssetError> {
        if self.expressions.len() >= MAX_MOLANG_EXPRESSIONS {
            return Err(invalid("Molang expression count exceeds bound"));
        }
        let expression = Parser::new(source)?.parse()?;
        let index = self.expressions.len() as u32;
        self.expressions.push(expression);
        Ok(index)
    }

    pub fn add_name(&mut self, name: &str) -> Result<(), AssetError> {
        if name.is_empty() {
            return Err(invalid("empty Molang name"));
        }
        self.names.insert(name.into());
        Ok(())
    }

    pub fn finish(self) -> Result<MolangPayload, AssetError> {
        let mut symbol_set = BTreeSet::new();
        for expression in &self.expressions {
            expression.collect_symbols(&mut symbol_set);
        }
        symbol_set.extend(
            self.names
                .into_iter()
                .map(|name| (MolangSymbolKind::Name, name)),
        );
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
        let mut expressions = Vec::with_capacity(self.expressions.len());
        for expression in self.expressions {
            let first_op = ops.len() as u32;
            expression.emit(&indices, &mut ops)?;
            let op_count = ops.len() - first_op as usize;
            if op_count == 0 || op_count > MAX_MOLANG_OPS_PER_EXPRESSION {
                return Err(invalid("Molang operation count exceeds bound"));
            }
            let max_stack = calculate_stack(&ops[first_op as usize..])?;
            expressions.push(CompiledMolangExpression {
                first_op,
                op_count: op_count as u16,
                max_stack,
            });
        }
        if ops.len() > MAX_MOLANG_OPS {
            return Err(invalid("total Molang operation count exceeds bound"));
        }

        Ok(MolangPayload {
            symbols: symbols.into_boxed_slice(),
            expressions: expressions.into_boxed_slice(),
            ops: ops.into_boxed_slice(),
            collections: Box::new([]),
            collection_items: Box::new([]),
        })
    }
}

#[derive(Clone, Copy)]
enum Unary {
    Negate,
    Not,
}

#[derive(Clone, Copy)]
enum Binary {
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Equal,
    NotEqual,
    And,
    Or,
}

#[derive(Clone, Copy)]
enum Function {
    Abs,
    Ceil,
    Floor,
    Round,
    Sqrt,
    Sin,
    Cos,
    Min,
    Max,
    Clamp,
    Lerp,
}

#[derive(Clone)]
enum Expr {
    Constant(f32),
    Symbol(MolangSymbolKind, Box<str>),
    Unary(Unary, Box<Expr>),
    Binary(Binary, Box<Expr>, Box<Expr>),
    Ternary(Box<Expr>, Box<Expr>, Box<Expr>),
    Function(Function, Vec<Expr>),
}

impl Expr {
    fn collect_symbols(&self, symbols: &mut BTreeSet<(MolangSymbolKind, Box<str>)>) {
        match self {
            Self::Symbol(kind, identifier) => {
                symbols.insert((*kind, identifier.clone()));
            }
            Self::Unary(_, value) => value.collect_symbols(symbols),
            Self::Binary(_, left, right) => {
                left.collect_symbols(symbols);
                right.collect_symbols(symbols);
            }
            Self::Ternary(condition, yes, no) => {
                condition.collect_symbols(symbols);
                yes.collect_symbols(symbols);
                no.collect_symbols(symbols);
            }
            Self::Function(_, arguments) => {
                for argument in arguments {
                    argument.collect_symbols(symbols);
                }
            }
            Self::Constant(_) => {}
        }
    }

    fn emit(
        &self,
        symbols: &BTreeMap<(MolangSymbolKind, Box<str>), u32>,
        output: &mut Vec<MolangOp>,
    ) -> Result<(), AssetError> {
        match self {
            Self::Constant(value) => output.push(MolangOp::Push(scalar(*value)?)),
            Self::Symbol(kind, identifier) => {
                let index = symbols
                    .get(&(*kind, identifier.clone()))
                    .copied()
                    .ok_or_else(|| invalid("Molang symbol was not interned"))?;
                output.push(match kind {
                    MolangSymbolKind::Query => MolangOp::LoadQuery(index),
                    MolangSymbolKind::Variable | MolangSymbolKind::Temporary => {
                        MolangOp::LoadVariable(index)
                    }
                    MolangSymbolKind::Name => return Err(invalid("name used as runtime value")),
                });
            }
            Self::Unary(operator, value) => {
                value.emit(symbols, output)?;
                output.push(match operator {
                    Unary::Negate => MolangOp::Negate,
                    Unary::Not => MolangOp::Not,
                });
            }
            Self::Binary(operator, left, right) => {
                left.emit(symbols, output)?;
                right.emit(symbols, output)?;
                output.push(match operator {
                    Binary::Add => MolangOp::Add,
                    Binary::Subtract => MolangOp::Subtract,
                    Binary::Multiply => MolangOp::Multiply,
                    Binary::Divide => MolangOp::Divide,
                    Binary::Modulo => MolangOp::Modulo,
                    Binary::Less => MolangOp::Less,
                    Binary::LessEqual => MolangOp::LessEqual,
                    Binary::Greater => MolangOp::Greater,
                    Binary::GreaterEqual => MolangOp::GreaterEqual,
                    Binary::Equal => MolangOp::Equal,
                    Binary::NotEqual => MolangOp::NotEqual,
                    Binary::And => MolangOp::And,
                    Binary::Or => MolangOp::Or,
                });
            }
            Self::Ternary(condition, yes, no) => {
                condition.emit(symbols, output)?;
                yes.emit(symbols, output)?;
                no.emit(symbols, output)?;
                output.push(MolangOp::Select);
            }
            Self::Function(function, arguments) => {
                for argument in arguments {
                    argument.emit(symbols, output)?;
                }
                output.push(function.op());
            }
        }
        if output.len() > MAX_MOLANG_OPS {
            return Err(invalid("total Molang operation count exceeds bound"));
        }
        Ok(())
    }
}

impl Function {
    const fn arity(self) -> usize {
        match self {
            Self::Abs
            | Self::Ceil
            | Self::Floor
            | Self::Round
            | Self::Sqrt
            | Self::Sin
            | Self::Cos => 1,
            Self::Min | Self::Max => 2,
            Self::Clamp | Self::Lerp => 3,
        }
    }

    const fn op(self) -> MolangOp {
        match self {
            Self::Abs => MolangOp::Abs,
            Self::Ceil => MolangOp::Ceil,
            Self::Floor => MolangOp::Floor,
            Self::Round => MolangOp::Round,
            Self::Sqrt => MolangOp::Sqrt,
            Self::Sin => MolangOp::Sin,
            Self::Cos => MolangOp::Cos,
            Self::Min => MolangOp::Min,
            Self::Max => MolangOp::Max,
            Self::Clamp => MolangOp::Clamp,
            Self::Lerp => MolangOp::Lerp,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Number(f32),
    Identifier(Box<str>),
    LeftParen,
    RightParen,
    Comma,
    Question,
    Colon,
    Operator(&'static str),
    End,
}

struct Lexer<'a> {
    bytes: &'a [u8],
    cursor: usize,
}

impl<'a> Lexer<'a> {
    fn next(&mut self) -> Result<Token, AssetError> {
        while self
            .bytes
            .get(self.cursor)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.cursor += 1;
        }
        let Some(&byte) = self.bytes.get(self.cursor) else {
            return Ok(Token::End);
        };
        let punctuation = match byte {
            b'(' => Some(Token::LeftParen),
            b')' => Some(Token::RightParen),
            b',' => Some(Token::Comma),
            b'?' => Some(Token::Question),
            b':' => Some(Token::Colon),
            _ => None,
        };
        if let Some(token) = punctuation {
            self.cursor += 1;
            return Ok(token);
        }
        for (text, operator) in [
            (b"&&".as_slice(), "&&"),
            (b"||", "||"),
            (b"<=", "<="),
            (b">=", ">="),
            (b"==", "=="),
            (b"!=", "!="),
        ] {
            if self.bytes[self.cursor..].starts_with(text) {
                self.cursor += text.len();
                return Ok(Token::Operator(operator));
            }
        }
        if let Some(operator) = match byte {
            b'+' => Some("+"),
            b'-' => Some("-"),
            b'*' => Some("*"),
            b'/' => Some("/"),
            b'%' => Some("%"),
            b'<' => Some("<"),
            b'>' => Some(">"),
            b'!' => Some("!"),
            _ => None,
        } {
            self.cursor += 1;
            return Ok(Token::Operator(operator));
        }
        if byte.is_ascii_digit() || byte == b'.' {
            let start = self.cursor;
            self.cursor += 1;
            while self.bytes.get(self.cursor).is_some_and(|byte| {
                byte.is_ascii_digit() || matches!(byte, b'.' | b'e' | b'E' | b'+' | b'-')
            }) {
                if matches!(self.bytes[self.cursor], b'+' | b'-')
                    && !matches!(self.bytes[self.cursor - 1], b'e' | b'E')
                {
                    break;
                }
                self.cursor += 1;
            }
            let text = std::str::from_utf8(&self.bytes[start..self.cursor])
                .map_err(|_| invalid("Molang number is not UTF-8"))?;
            let value = text
                .parse::<f32>()
                .map_err(|_| invalid("invalid Molang numeric literal"))?;
            scalar(value)?;
            return Ok(Token::Number(value));
        }
        if byte.is_ascii_alphabetic() || byte == b'_' {
            let start = self.cursor;
            self.cursor += 1;
            while self
                .bytes
                .get(self.cursor)
                .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.'))
            {
                self.cursor += 1;
            }
            let identifier = std::str::from_utf8(&self.bytes[start..self.cursor])
                .map_err(|_| invalid("Molang identifier is not UTF-8"))?;
            return Ok(Token::Identifier(identifier.into()));
        }
        Err(invalid("unsupported token in Molang expression"))
    }
}

struct Parser<'a> {
    lexer: Lexer<'a>,
    current: Token,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str) -> Result<Self, AssetError> {
        if source.is_empty() || source.len() > MAX_EXPRESSION_BYTES {
            return Err(invalid("Molang expression size exceeds bound"));
        }
        let mut lexer = Lexer {
            bytes: source.as_bytes(),
            cursor: 0,
        };
        let current = lexer.next()?;
        Ok(Self { lexer, current })
    }

    fn parse(mut self) -> Result<Expr, AssetError> {
        let expression = self.parse_ternary(0)?;
        if self.current != Token::End {
            return Err(invalid("trailing or unsupported Molang syntax"));
        }
        Ok(expression)
    }

    fn bump(&mut self) -> Result<Token, AssetError> {
        let previous = std::mem::replace(&mut self.current, self.lexer.next()?);
        Ok(previous)
    }

    fn parse_ternary(&mut self, depth: usize) -> Result<Expr, AssetError> {
        self.check_depth(depth)?;
        let condition = self.parse_binary(1, depth + 1)?;
        if self.current != Token::Question {
            return Ok(condition);
        }
        self.bump()?;
        let yes = self.parse_ternary(depth + 1)?;
        if self.current != Token::Colon {
            return Err(invalid("Molang ternary is missing `:`"));
        }
        self.bump()?;
        let no = self.parse_ternary(depth + 1)?;
        fold_ternary(condition, yes, no)
    }

    fn parse_binary(&mut self, min_precedence: u8, depth: usize) -> Result<Expr, AssetError> {
        self.check_depth(depth)?;
        let mut left = self.parse_unary(depth + 1)?;
        loop {
            let Token::Operator(operator) = self.current else {
                break;
            };
            let Some((precedence, binary)) = binary_operator(operator) else {
                break;
            };
            if precedence < min_precedence {
                break;
            }
            self.bump()?;
            let right = self.parse_binary(precedence + 1, depth + 1)?;
            left = fold_binary(binary, left, right)?;
        }
        Ok(left)
    }

    fn parse_unary(&mut self, depth: usize) -> Result<Expr, AssetError> {
        self.check_depth(depth)?;
        if let Token::Operator(operator @ ("-" | "!")) = self.current {
            self.bump()?;
            return fold_unary(
                if operator == "-" {
                    Unary::Negate
                } else {
                    Unary::Not
                },
                self.parse_unary(depth + 1)?,
            );
        }
        self.parse_primary(depth + 1)
    }

    fn parse_primary(&mut self, depth: usize) -> Result<Expr, AssetError> {
        self.check_depth(depth)?;
        match self.bump()? {
            Token::Number(value) => Ok(Expr::Constant(value)),
            Token::Identifier(identifier) if identifier.as_ref() == "true" => {
                Ok(Expr::Constant(1.0))
            }
            Token::Identifier(identifier) if identifier.as_ref() == "false" => {
                Ok(Expr::Constant(0.0))
            }
            Token::Identifier(identifier) if self.current == Token::LeftParen => {
                let function = parse_function(&identifier)?;
                self.bump()?;
                let mut arguments = Vec::new();
                if self.current != Token::RightParen {
                    loop {
                        arguments.push(self.parse_ternary(depth + 1)?);
                        if self.current != Token::Comma {
                            break;
                        }
                        self.bump()?;
                    }
                }
                if self.current != Token::RightParen || arguments.len() != function.arity() {
                    return Err(invalid("Molang function has invalid arity"));
                }
                self.bump()?;
                fold_function(function, arguments)
            }
            Token::Identifier(identifier) => {
                let kind = if identifier.starts_with("query.") {
                    if !matches!(
                        identifier.as_ref(),
                        "query.anim_time"
                            | "query.life_time"
                            | "query.modified_move_speed"
                            | "query.ground_speed"
                            | "query.is_on_ground"
                            | "query.is_moving"
                            | "query.is_sprinting"
                            | "query.is_sneaking"
                            | "query.is_sleeping"
                            | "query.body_y_rotation"
                            | "query.head_y_rotation"
                            | "query.target_x_rotation"
                    ) {
                        return Err(invalid("unlisted Molang query"));
                    }
                    MolangSymbolKind::Query
                } else if identifier.starts_with("variable.") {
                    validate_slot(&identifier, "variable.")?;
                    MolangSymbolKind::Variable
                } else if identifier.starts_with("temp.") {
                    validate_slot(&identifier, "temp.")?;
                    MolangSymbolKind::Temporary
                } else {
                    return Err(invalid("unlisted Molang identifier"));
                };
                Ok(Expr::Symbol(kind, identifier))
            }
            Token::LeftParen => {
                let expression = self.parse_ternary(depth + 1)?;
                if self.current != Token::RightParen {
                    return Err(invalid("unclosed Molang parenthesis"));
                }
                self.bump()?;
                Ok(expression)
            }
            _ => Err(invalid("expected Molang expression value")),
        }
    }

    fn check_depth(&self, depth: usize) -> Result<(), AssetError> {
        if depth > MAX_PARSE_DEPTH {
            Err(invalid("Molang parse depth exceeds bound"))
        } else {
            Ok(())
        }
    }
}

fn validate_slot(identifier: &str, prefix: &str) -> Result<(), AssetError> {
    let valid = identifier.strip_prefix(prefix).is_some_and(|slot| {
        !slot.is_empty()
            && slot
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    });
    if valid {
        Ok(())
    } else {
        Err(invalid("invalid Molang variable or temporary slot"))
    }
}

fn parse_function(identifier: &str) -> Result<Function, AssetError> {
    match identifier {
        "math.abs" => Ok(Function::Abs),
        "math.ceil" => Ok(Function::Ceil),
        "math.floor" => Ok(Function::Floor),
        "math.round" => Ok(Function::Round),
        "math.sqrt" => Ok(Function::Sqrt),
        "math.sin" => Ok(Function::Sin),
        "math.cos" => Ok(Function::Cos),
        "math.min" => Ok(Function::Min),
        "math.max" => Ok(Function::Max),
        "math.clamp" => Ok(Function::Clamp),
        "math.lerp" => Ok(Function::Lerp),
        _ => Err(invalid("unsupported Molang function")),
    }
}

fn binary_operator(operator: &str) -> Option<(u8, Binary)> {
    Some(match operator {
        "||" => (1, Binary::Or),
        "&&" => (2, Binary::And),
        "==" => (3, Binary::Equal),
        "!=" => (3, Binary::NotEqual),
        "<" => (4, Binary::Less),
        "<=" => (4, Binary::LessEqual),
        ">" => (4, Binary::Greater),
        ">=" => (4, Binary::GreaterEqual),
        "+" => (5, Binary::Add),
        "-" => (5, Binary::Subtract),
        "*" => (6, Binary::Multiply),
        "/" => (6, Binary::Divide),
        "%" => (6, Binary::Modulo),
        _ => return None,
    })
}

fn fold_unary(operator: Unary, value: Expr) -> Result<Expr, AssetError> {
    if let Expr::Constant(value) = value {
        let result = match operator {
            Unary::Negate => -value,
            Unary::Not => bool_value(value == 0.0),
        };
        scalar(result)?;
        Ok(Expr::Constant(result))
    } else {
        Ok(Expr::Unary(operator, Box::new(value)))
    }
}

fn fold_binary(operator: Binary, left: Expr, right: Expr) -> Result<Expr, AssetError> {
    if let (Expr::Constant(left), Expr::Constant(right)) = (&left, &right) {
        let result = match operator {
            Binary::Add => left + right,
            Binary::Subtract => left - right,
            Binary::Multiply => left * right,
            Binary::Divide => {
                if *right == 0.0 {
                    0.0
                } else {
                    left / right
                }
            }
            Binary::Modulo => {
                if *right == 0.0 {
                    0.0
                } else {
                    left % right
                }
            }
            Binary::Less => bool_value(left < right),
            Binary::LessEqual => bool_value(left <= right),
            Binary::Greater => bool_value(left > right),
            Binary::GreaterEqual => bool_value(left >= right),
            Binary::Equal => bool_value(left == right),
            Binary::NotEqual => bool_value(left != right),
            Binary::And => bool_value(*left != 0.0 && *right != 0.0),
            Binary::Or => bool_value(*left != 0.0 || *right != 0.0),
        };
        scalar(result)?;
        Ok(Expr::Constant(result))
    } else {
        Ok(Expr::Binary(operator, Box::new(left), Box::new(right)))
    }
}

fn fold_ternary(condition: Expr, yes: Expr, no: Expr) -> Result<Expr, AssetError> {
    if let Expr::Constant(condition) = condition {
        Ok(if condition != 0.0 { yes } else { no })
    } else {
        Ok(Expr::Ternary(
            Box::new(condition),
            Box::new(yes),
            Box::new(no),
        ))
    }
}

fn fold_function(function: Function, arguments: Vec<Expr>) -> Result<Expr, AssetError> {
    let values = arguments
        .iter()
        .map(|argument| match argument {
            Expr::Constant(value) => Some(*value),
            _ => None,
        })
        .collect::<Option<Vec<_>>>();
    let Some(values) = values else {
        return Ok(Expr::Function(function, arguments));
    };
    let result = match function {
        Function::Abs => values[0].abs(),
        Function::Ceil => values[0].ceil(),
        Function::Floor => values[0].floor(),
        Function::Round => values[0].round(),
        Function::Sqrt => values[0].max(0.0).sqrt(),
        Function::Sin => values[0].to_radians().sin(),
        Function::Cos => values[0].to_radians().cos(),
        Function::Min => values[0].min(values[1]),
        Function::Max => values[0].max(values[1]),
        Function::Clamp => values[0].clamp(values[1].min(values[2]), values[1].max(values[2])),
        Function::Lerp => values[0] + (values[1] - values[0]) * values[2],
    };
    scalar(result)?;
    Ok(Expr::Constant(result))
}

fn calculate_stack(ops: &[MolangOp]) -> Result<u8, AssetError> {
    let mut depth = 0usize;
    let mut maximum = 0usize;
    for op in ops {
        let (required, delta) = match op {
            MolangOp::Push(_) | MolangOp::LoadQuery(_) | MolangOp::LoadVariable(_) => (0, 1),
            MolangOp::Negate
            | MolangOp::Not
            | MolangOp::Abs
            | MolangOp::Ceil
            | MolangOp::Floor
            | MolangOp::Round
            | MolangOp::Sqrt
            | MolangOp::Sin
            | MolangOp::Cos
            | MolangOp::SelectCollection(_) => (1, 0),
            MolangOp::Select | MolangOp::Clamp | MolangOp::Lerp => (3, -2),
            _ => (2, -1),
        };
        if depth < required {
            return Err(invalid("compiled Molang stack underflows"));
        }
        depth = depth.saturating_add_signed(delta);
        maximum = maximum.max(depth);
    }
    if depth != 1 || maximum == 0 || maximum > MAX_MOLANG_STACK_DEPTH as usize {
        return Err(invalid("compiled Molang stack exceeds bound"));
    }
    Ok(maximum as u8)
}

fn scalar(value: f32) -> Result<EntityGeometryScalar, AssetError> {
    EntityGeometryScalar::new(value).ok_or_else(|| invalid("non-finite or excessive scalar"))
}

const fn bool_value(value: bool) -> f32 {
    if value { 1.0 } else { 0.0 }
}
