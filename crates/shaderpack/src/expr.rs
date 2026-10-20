//! The OptiFine/Iris expression language.
//!
//! One grammar serves `program.*.enabled` rules, `#if`/`#elif` conditions in
//! `shaders.properties` and the `uniform.*`/`variable.*` custom uniform expressions.
//! Parsing never panics on malformed text and bounds nesting so hostile packs cannot
//! exhaust the stack.

use std::fmt;

/// Deepest expression tree accepted; bounds evaluation and drop recursion.
const MAX_TREE_DEPTH: u32 = 256;
/// Deepest parser recursion (parentheses, prefix operators, call arguments) accepted.
const MAX_NESTING: u32 = 64;

/// A parsed expression.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// Numeric literal; integer, float and hex literals all become `f64`.
    Number(f64),
    /// `true` or `false`.
    Bool(bool),
    /// Identifier, including dotted member paths such as `cameraPosition.y`.
    Ident(String),
    /// `defined NAME` / `defined(NAME)` (preprocessor conditionals only).
    Defined(String),
    /// Prefix operator applied to its operand.
    Unary(UnaryOp, Box<Expr>),
    /// Infix operator applied to its left and right operands.
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
    /// Function call, e.g. `clamp(x, 0.0, 1.0)`, `if(c, a, b)`, `vec3(1, 2, 3)`.
    Call(String, Vec<Expr>),
}

/// Prefix operators. A prefix `+` is accepted by the parser and leaves no node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    /// Arithmetic negation `-x`.
    Neg,
    /// Logical not `!x`: 1 when the operand is zero, else 0.
    Not,
}

/// Infix operators, from `*` (binds tightest) to `||` (binds loosest).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    /// `*`
    Mul,
    /// `/`
    Div,
    /// `%` (floating-point remainder, sign of the dividend).
    Rem,
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
    /// `==`
    Eq,
    /// `!=`
    Ne,
    /// `&&` (short-circuiting).
    And,
    /// `||` (short-circuiting).
    Or,
}

/// A parse or evaluation failure with a human-readable message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExprError {
    /// What went wrong, suitable for a pack diagnostic.
    pub message: String,
}

impl ExprError {
    /// Creates an expression error from a diagnostic message.
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for ExprError {
    /// Writes the expression error message.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ExprError {}

/// Name lookup used by [`Expr::eval`]: `Some(value)` for a defined name, `None` otherwise.
type Lookup<'a> = &'a dyn Fn(&str) -> Option<f64>;

impl Expr {
    /// Parses one expression; trailing tokens are an error.
    ///
    /// Grammar from loosest to tightest: `||`, `&&`, `== !=`, `< <= > >=`, `+ -`,
    /// `* / %`, prefix `- ! +`, then primaries: numbers (`1`, `1.`, `.5`, `1e3`, `0x10`,
    /// with `u`/`U`/`f`/`F` suffixes ignored), `true`/`false`, `defined NAME`,
    /// `defined(NAME)`, dotted identifiers, calls and parenthesised expressions.
    pub fn parse(text: &str) -> Result<Expr, ExprError> {
        let tokens = lex(text)?;
        let mut parser = Parser {
            tokens,
            pos: 0,
            nesting: 0,
        };
        let (expr, _) = parser.parse_binary(0)?;
        match parser.peek() {
            None => Ok(expr),
            Some(token) => Err(ExprError::new(format!(
                "unexpected trailing {}",
                token.describe()
            ))),
        }
    }

    /// Every identifier referenced (deduplicated, in first-use order), excluding function
    /// names. Names tested with `defined` count as referenced.
    pub fn identifiers(&self) -> Vec<&str> {
        let mut out = Vec::new();
        self.collect_identifiers(&mut out);
        out
    }

    /// Appends referenced names once, preserving their first-use order.
    fn collect_identifiers<'a>(&'a self, out: &mut Vec<&'a str>) {
        match self {
            Expr::Number(_) | Expr::Bool(_) => {}
            Expr::Ident(name) | Expr::Defined(name) => {
                if !out.contains(&name.as_str()) {
                    out.push(name);
                }
            }
            Expr::Unary(_, operand) => operand.collect_identifiers(out),
            Expr::Binary(_, lhs, rhs) => {
                lhs.collect_identifiers(out);
                rhs.collect_identifiers(out);
            }
            Expr::Call(_, args) => args.iter().for_each(|arg| arg.collect_identifiers(out)),
        }
    }

    /// Scalar evaluation. `lookup(name)` returns `Some(value)` for a defined name
    /// (an empty-valued define or enabled boolean option is `1.0`) and `None` if undefined.
    /// Undefined identifiers evaluate to 0 (C preprocessor rule); `Defined(n)` is 1/0 by lookup.
    /// Booleans are 1.0/0.0; `&&`/`||`/`!` treat non-zero as true and short-circuit.
    /// Supported functions: `if(c, a, b[, c2, b2...], else)` (OptiFine multi-branch if),
    /// `min`, `max` (two or more arguments), `clamp`, `abs`, `floor`, `ceil`, `sin`, `cos`,
    /// `pow`, `sqrt`, `between(x, lo, hi)`, `in(x, v1, v2, ...)`, `equals(a, b, epsilon)`.
    /// Any other call, or a wrong argument count, is an error.
    pub fn eval(&self, lookup: &dyn Fn(&str) -> Option<f64>) -> Result<f64, ExprError> {
        Ok(match self {
            Expr::Number(value) => *value,
            Expr::Bool(value) => bool_value(*value),
            Expr::Ident(name) => lookup(name.as_str()).unwrap_or(0.0),
            Expr::Defined(name) => bool_value(lookup(name.as_str()).is_some()),
            Expr::Unary(op, operand) => {
                let value = operand.eval(lookup)?;
                match op {
                    UnaryOp::Neg => -value,
                    UnaryOp::Not => bool_value(value == 0.0),
                }
            }
            Expr::Binary(op, lhs, rhs) => eval_binary(*op, lhs, rhs, lookup)?,
            Expr::Call(name, args) => eval_call(name, args, lookup)?,
        })
    }
}

/// Parses a lone numeric literal with an optional sign (`12`, `-0.5`, `0x1F`, `2u`),
/// as used for macro values in `#if` conditions. Anything else is `None`.
pub(crate) fn parse_number_literal(text: &str) -> Option<f64> {
    let tokens = lex(text).ok()?;
    match tokens.as_slice() {
        [Token::Number(value)] | [Token::Plus, Token::Number(value)] => Some(*value),
        [Token::Minus, Token::Number(value)] => Some(-value),
        _ => None,
    }
}

/// Numeric value of a defined macro in a condition: its number literal, 1 when empty, else
/// 0 (a non-numeric expansion is an undefined identifier to the C preprocessor).
pub(crate) fn macro_value(value: &str) -> f64 {
    let value = value.trim();
    if value.is_empty() {
        1.0
    } else {
        parse_number_literal(value).unwrap_or(0.0)
    }
}

/// Converts a boolean result to the numeric value used by expressions.
fn bool_value(value: bool) -> f64 {
    if value { 1.0 } else { 0.0 }
}

/// Evaluates a binary expression, short-circuiting logical operators.
fn eval_binary(op: BinaryOp, lhs: &Expr, rhs: &Expr, lookup: Lookup<'_>) -> Result<f64, ExprError> {
    let l = lhs.eval(lookup)?;
    Ok(match op {
        BinaryOp::And => bool_value(l != 0.0 && rhs.eval(lookup)? != 0.0),
        BinaryOp::Or => bool_value(l != 0.0 || rhs.eval(lookup)? != 0.0),
        BinaryOp::Mul => l * rhs.eval(lookup)?,
        BinaryOp::Div => l / rhs.eval(lookup)?,
        BinaryOp::Rem => l % rhs.eval(lookup)?,
        BinaryOp::Add => l + rhs.eval(lookup)?,
        BinaryOp::Sub => l - rhs.eval(lookup)?,
        BinaryOp::Lt => bool_value(l < rhs.eval(lookup)?),
        BinaryOp::Le => bool_value(l <= rhs.eval(lookup)?),
        BinaryOp::Gt => bool_value(l > rhs.eval(lookup)?),
        BinaryOp::Ge => bool_value(l >= rhs.eval(lookup)?),
        BinaryOp::Eq => bool_value(l == rhs.eval(lookup)?),
        BinaryOp::Ne => bool_value(l != rhs.eval(lookup)?),
    })
}

/// Evaluates a supported scalar function or reports its name or arity error.
fn eval_call(name: &str, args: &[Expr], lookup: Lookup<'_>) -> Result<f64, ExprError> {
    match name {
        "if" => eval_if(args, lookup),
        "min" => variadic_args(name, args, 2, lookup)
            .map(|values| values.into_iter().fold(f64::INFINITY, f64::min)),
        "max" => variadic_args(name, args, 2, lookup)
            .map(|values| values.into_iter().fold(f64::NEG_INFINITY, f64::max)),
        // GLSL clamp: min(max(x, lo), hi); never panics when lo > hi.
        "clamp" => fixed_args::<3>(name, args, lookup).map(|[x, lo, hi]| x.max(lo).min(hi)),
        "abs" => fixed_args::<1>(name, args, lookup).map(|[x]| x.abs()),
        "floor" => fixed_args::<1>(name, args, lookup).map(|[x]| x.floor()),
        "ceil" => fixed_args::<1>(name, args, lookup).map(|[x]| x.ceil()),
        "sin" => fixed_args::<1>(name, args, lookup).map(|[x]| x.sin()),
        "cos" => fixed_args::<1>(name, args, lookup).map(|[x]| x.cos()),
        "sqrt" => fixed_args::<1>(name, args, lookup).map(|[x]| x.sqrt()),
        "pow" => fixed_args::<2>(name, args, lookup).map(|[x, y]| x.powf(y)),
        "between" => fixed_args::<3>(name, args, lookup)
            .map(|[x, lo, hi]| bool_value((lo..=hi).contains(&x))),
        "equals" => fixed_args::<3>(name, args, lookup)
            .map(|[a, b, epsilon]| bool_value((a - b).abs() <= epsilon)),
        "in" => {
            let values = variadic_args(name, args, 2, lookup)?;
            let found = values
                .split_first()
                .is_some_and(|(first, rest)| rest.contains(first));
            Ok(bool_value(found))
        }
        _ => Err(ExprError::new(format!(
            "function `{name}` is not supported in scalar evaluation"
        ))),
    }
}

/// OptiFine multi-branch `if(c1, v1, c2, v2, ..., else)`; only the taken branch is evaluated.
fn eval_if(args: &[Expr], lookup: Lookup<'_>) -> Result<f64, ExprError> {
    let arity_error = || {
        ExprError::new(format!(
            "`if` expects an odd number of at least 3 arguments, found {}",
            args.len()
        ))
    };
    let Some((otherwise, branches)) = args.split_last() else {
        return Err(arity_error());
    };
    let (pairs, rest) = branches.as_chunks::<2>();
    if branches.is_empty() || !rest.is_empty() {
        return Err(arity_error());
    }
    for [condition, value] in pairs {
        if condition.eval(lookup)? != 0.0 {
            return value.eval(lookup);
        }
    }
    otherwise.eval(lookup)
}

/// Evaluates exactly N arguments, reporting a mismatched argument count.
fn fixed_args<const N: usize>(
    name: &str,
    args: &[Expr],
    lookup: Lookup<'_>,
) -> Result<[f64; N], ExprError> {
    if args.len() != N {
        return Err(ExprError::new(format!(
            "`{name}` expects {N} argument(s), found {}",
            args.len()
        )));
    }
    let mut values = [0.0; N];
    for (slot, arg) in values.iter_mut().zip(args) {
        *slot = arg.eval(lookup)?;
    }
    Ok(values)
}

/// Evaluates arguments after checking the minimum argument count.
fn variadic_args(
    name: &str,
    args: &[Expr],
    min: usize,
    lookup: Lookup<'_>,
) -> Result<Vec<f64>, ExprError> {
    if args.len() < min {
        return Err(ExprError::new(format!(
            "`{name}` expects at least {min} arguments, found {}",
            args.len()
        )));
    }
    args.iter().map(|arg| arg.eval(lookup)).collect()
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f64),
    Ident(String),
    LParen,
    RParen,
    Comma,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Bang,
    Lt,
    Le,
    Gt,
    Ge,
    EqEq,
    NotEq,
    AndAnd,
    OrOr,
}

impl Token {
    /// Describes a token for a parser diagnostic.
    fn describe(&self) -> String {
        let symbol = match self {
            Token::Number(value) => return format!("number `{value}`"),
            Token::Ident(name) => return format!("`{name}`"),
            Token::LParen => "(",
            Token::RParen => ")",
            Token::Comma => ",",
            Token::Plus => "+",
            Token::Minus => "-",
            Token::Star => "*",
            Token::Slash => "/",
            Token::Percent => "%",
            Token::Bang => "!",
            Token::Lt => "<",
            Token::Le => "<=",
            Token::Gt => ">",
            Token::Ge => ">=",
            Token::EqEq => "==",
            Token::NotEq => "!=",
            Token::AndAnd => "&&",
            Token::OrOr => "||",
        };
        format!("`{symbol}`")
    }
}

/// Tokenizes an expression, reporting unsupported characters and invalid numbers.
fn lex(text: &str) -> Result<Vec<Token>, ExprError> {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while let Some(&c) = bytes.get(i) {
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let next = bytes.get(i + 1).copied();
        if c.is_ascii_digit() || (c == b'.' && next.is_some_and(|b| b.is_ascii_digit())) {
            let (value, end) = lex_number(text, i)?;
            tokens.push(Token::Number(value));
            i = end;
            continue;
        }
        if c.is_ascii_alphabetic() || c == b'_' {
            let end = ident_end(bytes, i);
            tokens.push(Token::Ident(text[i..end].to_owned()));
            i = end;
            continue;
        }
        let (token, len) = match (c, next) {
            (b'(', _) => (Token::LParen, 1),
            (b')', _) => (Token::RParen, 1),
            (b',', _) => (Token::Comma, 1),
            (b'+', _) => (Token::Plus, 1),
            (b'-', _) => (Token::Minus, 1),
            (b'*', _) => (Token::Star, 1),
            (b'/', _) => (Token::Slash, 1),
            (b'%', _) => (Token::Percent, 1),
            (b'<', Some(b'=')) => (Token::Le, 2),
            (b'<', _) => (Token::Lt, 1),
            (b'>', Some(b'=')) => (Token::Ge, 2),
            (b'>', _) => (Token::Gt, 1),
            (b'=', Some(b'=')) => (Token::EqEq, 2),
            (b'!', Some(b'=')) => (Token::NotEq, 2),
            (b'!', _) => (Token::Bang, 1),
            (b'&', Some(b'&')) => (Token::AndAnd, 2),
            (b'|', Some(b'|')) => (Token::OrOr, 2),
            _ => {
                let found = text[i..].chars().next().unwrap_or('?');
                return Err(ExprError::new(format!("unexpected character `{found}`")));
            }
        };
        tokens.push(token);
        i += len;
    }
    Ok(tokens)
}

/// End of an identifier starting at `start`: a name followed by `.`-separated parts,
/// which may start with digits (`gbufferModelViewInverse.0.1`).
fn ident_end(bytes: &[u8], start: usize) -> usize {
    let is_part = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut end = start;
    while bytes.get(end).is_some_and(|&b| is_part(b)) {
        end += 1;
    }
    while bytes.get(end) == Some(&b'.') && bytes.get(end + 1).is_some_and(|&b| is_part(b)) {
        end += 1;
        while bytes.get(end).is_some_and(|&b| is_part(b)) {
            end += 1;
        }
    }
    end
}

/// Finds the first byte after a run of decimal digits.
fn digits_end(bytes: &[u8], mut end: usize) -> usize {
    while bytes.get(end).is_some_and(u8::is_ascii_digit) {
        end += 1;
    }
    end
}

/// Lexes the number starting at `start`, returning its value and end offset.
fn lex_number(text: &str, start: usize) -> Result<(f64, usize), ExprError> {
    let bytes = text.as_bytes();
    let is_hex =
        bytes.get(start) == Some(&b'0') && matches!(bytes.get(start + 1), Some(b'x' | b'X'));
    let (value, mut end) = if is_hex {
        let digits_start = start + 2;
        let mut end = digits_start;
        while bytes.get(end).is_some_and(u8::is_ascii_hexdigit) {
            end += 1;
        }
        let value = u64::from_str_radix(&text[digits_start..end], 16)
            .map_err(|_| ExprError::new(format!("invalid hex literal `{}`", &text[start..end])))?;
        if matches!(bytes.get(end), Some(b'u' | b'U')) {
            end += 1;
        }
        (value as f64, end)
    } else {
        let mut end = digits_end(bytes, start);
        if bytes.get(end) == Some(&b'.') {
            end = digits_end(bytes, end + 1);
        }
        if matches!(bytes.get(end), Some(b'e' | b'E')) {
            let mut exponent = end + 1;
            if matches!(bytes.get(exponent), Some(b'+' | b'-')) {
                exponent += 1;
            }
            if bytes.get(exponent).is_some_and(u8::is_ascii_digit) {
                end = digits_end(bytes, exponent);
            }
        }
        let value = text[start..end]
            .parse::<f64>()
            .map_err(|_| ExprError::new(format!("invalid number `{}`", &text[start..end])))?;
        if matches!(bytes.get(end), Some(b'u' | b'U' | b'f' | b'F')) {
            end += 1;
        }
        (value, end)
    };
    if bytes
        .get(end)
        .is_some_and(|&b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.')
    {
        end = ident_end(bytes, end);
        return Err(ExprError::new(format!(
            "invalid number literal `{}`",
            &text[start..end]
        )));
    }
    Ok((value, end))
}

/// An expression together with its tree depth (a leaf is 1).
type Parsed = (Expr, u32);

/// Binding level of the prefix operators, one past the tightest binary level.
const UNARY_LEVEL: u8 = 6;

/// Returns the operator only when it belongs to the requested precedence level.
fn binary_op(token: &Token, level: u8) -> Option<BinaryOp> {
    let (op, op_level) = match token {
        Token::OrOr => (BinaryOp::Or, 0),
        Token::AndAnd => (BinaryOp::And, 1),
        Token::EqEq => (BinaryOp::Eq, 2),
        Token::NotEq => (BinaryOp::Ne, 2),
        Token::Lt => (BinaryOp::Lt, 3),
        Token::Le => (BinaryOp::Le, 3),
        Token::Gt => (BinaryOp::Gt, 3),
        Token::Ge => (BinaryOp::Ge, 3),
        Token::Plus => (BinaryOp::Add, 4),
        Token::Minus => (BinaryOp::Sub, 4),
        Token::Star => (BinaryOp::Mul, 5),
        Token::Slash => (BinaryOp::Div, 5),
        Token::Percent => (BinaryOp::Rem, 5),
        _ => return None,
    };
    (op_level == level).then_some(op)
}

/// Wraps `expr` as a node one level above its deepest child, enforcing [`MAX_TREE_DEPTH`].
fn node(expr: Expr, child_depth: u32) -> Result<Parsed, ExprError> {
    let depth = child_depth + 1;
    if depth > MAX_TREE_DEPTH {
        return Err(ExprError::new("expression is nested too deeply"));
    }
    Ok((expr, depth))
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    /// Current recursion depth through [`Parser::parse_unary`].
    nesting: u32,
}

impl Parser {
    /// Borrows the next token without advancing the parser.
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    /// Consumes the next token, returning None at the end of input.
    fn next_token(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.pos).cloned();
        if token.is_some() {
            self.pos += 1;
        }
        token
    }

    /// Consumes the next token only when it matches the requested token.
    fn eat(&mut self, token: &Token) -> bool {
        let matched = self.peek() == Some(token);
        if matched {
            self.pos += 1;
        }
        matched
    }

    /// Consumes a required token or reports what appeared in its place.
    fn expect(&mut self, token: &Token, what: &str) -> Result<(), ExprError> {
        if self.eat(token) {
            return Ok(());
        }
        let found = self
            .peek()
            .map_or_else(|| "end of input".to_owned(), Token::describe);
        Err(ExprError::new(format!("expected {what}, found {found}")))
    }

    /// Left-associative binary operators at `level` (0 = `||`).
    fn parse_binary(&mut self, level: u8) -> Result<Parsed, ExprError> {
        if level >= UNARY_LEVEL {
            return self.parse_unary();
        }
        let mut lhs = self.parse_binary(level + 1)?;
        while let Some(op) = self.peek().and_then(|token| binary_op(token, level)) {
            self.pos += 1;
            let rhs = self.parse_binary(level + 1)?;
            let depth = lhs.1.max(rhs.1);
            lhs = node(Expr::Binary(op, Box::new(lhs.0), Box::new(rhs.0)), depth)?;
        }
        Ok(lhs)
    }

    /// Every recursive path (prefix operators, parentheses, call arguments) passes
    /// through here, so `nesting` bounds the parser's stack use.
    fn parse_unary(&mut self) -> Result<Parsed, ExprError> {
        self.nesting += 1;
        let result = if self.nesting > MAX_NESTING {
            Err(ExprError::new("expression is nested too deeply"))
        } else {
            self.parse_prefixed()
        };
        self.nesting -= 1;
        result
    }

    /// Parses a prefix operator and its operand, or a primary expression.
    fn parse_prefixed(&mut self) -> Result<Parsed, ExprError> {
        let op = match self.peek() {
            Some(Token::Plus) => {
                self.pos += 1;
                return self.parse_unary();
            }
            Some(Token::Minus) => UnaryOp::Neg,
            Some(Token::Bang) => UnaryOp::Not,
            _ => return self.parse_primary(),
        };
        self.pos += 1;
        let (operand, depth) = self.parse_unary()?;
        node(Expr::Unary(op, Box::new(operand)), depth)
    }

    /// Parses a literal, identifier, call or parenthesised expression.
    fn parse_primary(&mut self) -> Result<Parsed, ExprError> {
        match self.next_token() {
            Some(Token::Number(value)) => Ok((Expr::Number(value), 1)),
            Some(Token::Ident(name)) => self.parse_ident(name),
            Some(Token::LParen) => {
                let inner = self.parse_binary(0)?;
                self.expect(&Token::RParen, "`)`")?;
                Ok(inner)
            }
            Some(token) => Err(ExprError::new(format!(
                "expected expression, found {}",
                token.describe()
            ))),
            None => Err(ExprError::new("expected expression, found end of input")),
        }
    }

    /// Parses a consumed name as a boolean, defined test, variable or function call.
    fn parse_ident(&mut self, name: String) -> Result<Parsed, ExprError> {
        match name.as_str() {
            "true" => return Ok((Expr::Bool(true), 1)),
            "false" => return Ok((Expr::Bool(false), 1)),
            "defined" => return self.parse_defined(),
            _ => {}
        }
        if !self.eat(&Token::LParen) {
            return Ok((Expr::Ident(name), 1));
        }
        let mut args = Vec::new();
        let mut depth = 0;
        if !self.eat(&Token::RParen) {
            loop {
                let (arg, arg_depth) = self.parse_binary(0)?;
                depth = depth.max(arg_depth);
                args.push(arg);
                if self.eat(&Token::RParen) {
                    break;
                }
                self.expect(&Token::Comma, "`,` or `)`")?;
            }
        }
        node(Expr::Call(name, args), depth)
    }

    /// Parses the macro name and optional parentheses following defined.
    fn parse_defined(&mut self) -> Result<Parsed, ExprError> {
        let parenthesised = self.eat(&Token::LParen);
        let Some(Token::Ident(name)) = self.next_token() else {
            return Err(ExprError::new("`defined` expects a macro name"));
        };
        if parenthesised {
            self.expect(&Token::RParen, "`)`")?;
        }
        Ok((Expr::Defined(name), 1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parses and evaluates a test expression using the supplied variable values.
    fn eval_with(text: &str, vars: &[(&str, f64)]) -> f64 {
        let expr = Expr::parse(text).unwrap_or_else(|err| panic!("parse `{text}`: {err}"));
        let lookup = |name: &str| {
            vars.iter()
                .find(|(var, _)| *var == name)
                .map(|(_, value)| *value)
        };
        expr.eval(&lookup)
            .unwrap_or_else(|err| panic!("eval `{text}`: {err}"))
    }

    /// Evaluates a test expression with no defined variables.
    fn eval(text: &str) -> f64 {
        eval_with(text, &[])
    }

    /// Builds a boxed identifier node for expression-tree assertions.
    fn ident(name: &str) -> Box<Expr> {
        Box::new(Expr::Ident(name.to_owned()))
    }

    #[test]
    fn multiplication_binds_tighter_than_addition() {
        assert_eq!(
            Expr::parse("a + b * c"),
            Ok(Expr::Binary(
                BinaryOp::Add,
                ident("a"),
                Box::new(Expr::Binary(BinaryOp::Mul, ident("b"), ident("c")))
            ))
        );
        assert_eq!(eval("1 + 2 * 3"), 7.0);
        assert_eq!(eval("(1 + 2) * 3"), 9.0);
        assert_eq!(eval("7 % 4 + 1"), 4.0);
    }

    #[test]
    fn binary_operators_are_left_associative() {
        assert_eq!(eval("8 - 2 - 1"), 5.0);
        assert_eq!(eval("8 / 4 / 2"), 1.0);
    }

    #[test]
    fn logical_and_comparison_precedence() {
        assert_eq!(eval("1 || 0 && 0"), 1.0);
        assert_eq!(eval("(1 || 0) && 0"), 0.0);
        assert_eq!(eval("1 + 1 == 2 && 3 > 2"), 1.0);
        assert_eq!(eval("1 < 2 == 1"), 1.0);
        assert_eq!(eval("2 >= 2 && 2 <= 2 && 1 != 2"), 1.0);
        assert_eq!(eval("!0 && !(1 > 2)"), 1.0);
        assert_eq!(eval("-2 * -3"), 6.0);
        assert_eq!(eval("+4 - -1"), 5.0);
        assert_eq!(eval("true && !false"), 1.0);
    }

    #[test]
    fn number_literal_forms() {
        assert_eq!(eval("1."), 1.0);
        assert_eq!(eval(".5"), 0.5);
        assert_eq!(eval("1e3"), 1000.0);
        assert_eq!(eval("2.5e-1"), 0.25);
        assert_eq!(eval("0x10"), 16.0);
        assert_eq!(eval("0x1Fu"), 31.0);
        assert_eq!(eval("3u + 1U"), 4.0);
        assert_eq!(eval("0.5f * 2F"), 1.0);
        assert!(Expr::parse("12abc").is_err());
        assert!(Expr::parse("0x").is_err());
    }

    #[test]
    fn dotted_identifiers_are_single_names() {
        assert_eq!(
            eval_with(
                "eyePos.y * matrixA.1.2",
                &[("eyePos.y", 3.0), ("matrixA.1.2", 2.0)]
            ),
            6.0
        );
        assert_eq!(Expr::parse("a.b.c"), Ok(Expr::Ident("a.b.c".to_owned())));
    }

    #[test]
    fn defined_forms_and_undefined_names() {
        let vars = [("FOO", 1.0), ("ZERO", 0.0)];
        assert_eq!(eval_with("defined FOO", &vars), 1.0);
        assert_eq!(eval_with("defined(ZERO)", &vars), 1.0);
        assert_eq!(eval_with("defined ( BAR )", &vars), 0.0);
        assert_eq!(eval_with("!defined BAR && FOO", &vars), 1.0);
        assert_eq!(eval_with("BAR + 2", &vars), 2.0);
        assert!(Expr::parse("defined").is_err());
        assert!(Expr::parse("defined(1)").is_err());
    }

    #[test]
    fn identifiers_are_deduplicated_and_skip_function_names() {
        let expr = Expr::parse("clamp(a + b, defined C, a) * vec3(b, d.x, 1)").expect("parses");
        assert_eq!(expr.identifiers(), ["a", "b", "C", "d.x"]);
    }

    #[test]
    fn scalar_functions() {
        assert_eq!(eval("min(3, 1, 2)"), 1.0);
        assert_eq!(eval("max(3, 1, 2)"), 3.0);
        assert_eq!(eval("clamp(5, 0, 2)"), 2.0);
        assert_eq!(eval("clamp(-1, 0, 2)"), 0.0);
        assert_eq!(eval("clamp(1, 3, 2)"), 2.0);
        assert_eq!(eval("abs(-2.5)"), 2.5);
        assert_eq!(eval("floor(1.7) + ceil(1.2)"), 3.0);
        assert_eq!(eval("sin(0) + cos(0)"), 1.0);
        assert_eq!(eval("pow(2, 10)"), 1024.0);
        assert_eq!(eval("sqrt(16)"), 4.0);
        assert_eq!(eval("between(2, 1, 3) + between(4, 1, 3)"), 1.0);
        assert_eq!(eval("in(3, 1, 2, 3)"), 1.0);
        assert_eq!(eval("in(4, 1, 2, 3)"), 0.0);
        assert_eq!(eval("equals(1.0, 1.05, 0.1) + equals(1, 2, 0.5)"), 1.0);
    }

    #[test]
    fn multi_branch_if_takes_first_true_branch() {
        assert_eq!(eval("if(1, 10, 20)"), 10.0);
        assert_eq!(eval("if(0, 10, 20)"), 20.0);
        assert_eq!(eval("if(0, 10, 1, 30, 40)"), 30.0);
        assert_eq!(eval("if(0, 10, 0, 30, 40)"), 40.0);
        // Untaken branches are not evaluated, so unsupported calls there are harmless.
        assert_eq!(eval("if(1, 5, vec2(1, 2))"), 5.0);
    }

    #[test]
    fn evaluation_errors() {
        let lookup = |_: &str| None;
        for text in [
            "frac(1.5)",
            "if(1, 2)",
            "if(1, 2, 3, 4)",
            "min(1)",
            "pow(2)",
            "in(1)",
        ] {
            let expr = Expr::parse(text).expect("parses");
            assert!(
                expr.eval(&lookup).is_err(),
                "`{text}` should fail to evaluate"
            );
        }
    }

    #[test]
    fn calls_with_any_name_parse() {
        assert_eq!(
            Expr::parse("vec2(x, 1)"),
            Ok(Expr::Call(
                "vec2".to_owned(),
                vec![Expr::Ident("x".to_owned()), Expr::Number(1.0)]
            ))
        );
        assert_eq!(
            Expr::parse("f()"),
            Ok(Expr::Call("f".to_owned(), Vec::new()))
        );
    }

    #[test]
    fn malformed_text_is_an_error() {
        for text in [
            "", "1 +", "(1", "1)", "1 2", "a = b", "a & b", "f(1,", "f(1 2)", "#",
        ] {
            assert!(Expr::parse(text).is_err(), "`{text}` should not parse");
        }
    }

    #[test]
    fn deep_nesting_is_rejected_without_overflow() {
        let parens = format!(
            "{}1{}",
            "(".repeat(MAX_NESTING as usize),
            ")".repeat(MAX_NESTING as usize)
        );
        assert!(Expr::parse(&parens).is_err());
        let nots = format!("{}1", "!".repeat(MAX_NESTING as usize));
        assert!(Expr::parse(&nots).is_err());
        let chain = format!("1{}", " + 1".repeat(MAX_TREE_DEPTH as usize));
        assert!(Expr::parse(&chain).is_err());
        let shallow = format!("1{}", " + 1".repeat(49));
        assert_eq!(eval(&shallow), 50.0);
    }

    #[test]
    fn number_literal_helper() {
        assert_eq!(parse_number_literal("12"), Some(12.0));
        assert_eq!(parse_number_literal(" -0.5 "), Some(-0.5));
        assert_eq!(parse_number_literal("0x1F"), Some(31.0));
        assert_eq!(parse_number_literal(""), None);
        assert_eq!(parse_number_literal("ABC"), None);
        assert_eq!(parse_number_literal("1 + 1"), None);
    }
}
