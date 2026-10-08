//! Molang subset particle definitions use: arithmetic, ternaries, `math.*`, `variable.*`,
//! `temp.*`, `query.*`, `return` and assignments. Variables are interned per effect so a
//! particle's state is a dense `Vec<f32>`; a NaN slot is "unassigned" and reads as 0.

use std::collections::HashMap;

/// Interned variable names; the builtin slots below are fixed for every effect.
#[derive(Clone, Debug, Default)]
pub struct Interner {
    names: Vec<Box<str>>,
    index: HashMap<Box<str>, u16>,
}

pub const V_PARTICLE_AGE: u16 = 0;
pub const V_PARTICLE_LIFETIME: u16 = 1;
pub const V_PARTICLE_RANDOM: u16 = 2; // through 5
pub const V_EMITTER_AGE: u16 = 6;
pub const V_EMITTER_LIFETIME: u16 = 7;
pub const V_EMITTER_RANDOM: u16 = 8; // through 11

const BUILTINS: [&str; 12] = [
    "particle_age",
    "particle_lifetime",
    "particle_random_1",
    "particle_random_2",
    "particle_random_3",
    "particle_random_4",
    "emitter_age",
    "emitter_lifetime",
    "emitter_random_1",
    "emitter_random_2",
    "emitter_random_3",
    "emitter_random_4",
];

/// Bound on interned names so a hostile definition cannot grow per-particle state.
pub const MAX_VARIABLES: usize = 256;

impl Interner {
    #[must_use]
    pub fn with_builtins() -> Self {
        let mut interner = Self::default();
        for name in BUILTINS {
            interner.intern(name);
        }
        interner
    }

    /// Slot for a variable name (without the `variable.` prefix), or `None` once full.
    pub fn intern(&mut self, name: &str) -> Option<u16> {
        if let Some(&slot) = self.index.get(name) {
            return Some(slot);
        }
        if self.names.len() >= MAX_VARIABLES {
            return None;
        }
        let slot = self.names.len() as u16;
        let boxed: Box<str> = name.into();
        self.names.push(boxed.clone());
        self.index.insert(boxed, slot);
        Some(slot)
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<u16> {
        self.index.get(name).copied()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.names.len()
    }
}

/// Strips `variable.`/`v.` and lowercases; `None` when `name` is not a variable reference.
#[must_use]
pub fn variable_key(name: &str) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    lower
        .strip_prefix("variable.")
        .or_else(|| lower.strip_prefix("v."))
        .map(str::to_owned)
}

/// Deterministic xorshift generator; one stream per emitter.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    #[must_use]
    pub fn new(seed: u64) -> Self {
        let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        Self(z | 1)
    }

    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x >> 32) as u32
    }

    /// Uniform in `[0, 1)`.
    pub fn unit(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }

    pub fn range(&mut self, low: f32, high: f32) -> f32 {
        low + (high - low) * self.unit()
    }
}

/// Query values the host supplies; unknown queries read 0.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Queries {
    pub is_in_water: f32,
    pub is_in_water_or_rain: f32,
    pub is_baby: f32,
    pub ground_speed: f32,
    pub frame_alpha: f32,
    /// Potion swirl colour, linear 0..1 RGBA.
    pub spell_color: [f32; 4],
}

#[derive(Clone, Copy, Debug)]
enum Query {
    IsInWater,
    IsInWaterOrRain,
    IsBaby,
    GroundSpeed,
    FrameAlpha,
    SpellColor(usize),
    Zero,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    And,
    Or,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Func {
    Abs,
    Acos,
    Asin,
    Atan,
    Atan2,
    Ceil,
    Clamp,
    Cos,
    Exp,
    Floor,
    InverseLerp,
    Lerp,
    LerpRotate,
    Ln,
    Max,
    Min,
    Mod,
    Pow,
    Random,
    RandomInteger,
    DieRoll,
    Round,
    Sign,
    Sin,
    Sqrt,
    Trunc,
    HermiteBlend,
    Zero,
}

#[derive(Clone, Debug)]
enum Expr {
    Num(f32),
    Var(u16),
    Temp(u8),
    Query(Query),
    Neg(Box<Expr>),
    Not(Box<Expr>),
    Bin(Op, Box<Expr>, Box<Expr>),
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
    Coalesce(Box<Expr>, Box<Expr>),
    Call(Func, Vec<Expr>),
    AssignVar(u16, Box<Expr>),
    AssignTemp(u8, Box<Expr>),
    Return(Box<Expr>),
}

/// A parsed expression or `;`-separated script.
#[derive(Clone, Debug)]
pub struct Program {
    stmts: Vec<Expr>,
    constant: Option<f32>,
}

const MAX_TEMPS: usize = 8;
const MAX_DEPTH: u32 = 48;
// Bounds flat AST chains too, including recursive evaluation and destruction.
const MAX_TOKENS: usize = 256;

impl Program {
    #[must_use]
    pub fn constant(value: f32) -> Self {
        Self {
            stmts: vec![Expr::Num(value)],
            constant: Some(value),
        }
    }

    /// Parses `source`; unparseable text yields `None` (vanilla evaluates it as 0).
    #[must_use]
    pub fn parse(source: &str, interner: &mut Interner) -> Option<Self> {
        let tokens = lex(source)?;
        if tokens.len() > MAX_TOKENS {
            return None;
        }
        let mut parser = Parser {
            tokens,
            at: 0,
            interner,
            temps: Vec::new(),
            depth: 0,
        };
        let mut stmts = Vec::new();
        while parser.at < parser.tokens.len() {
            if parser.eat(&Token::Semicolon) {
                continue;
            }
            stmts.push(parser.statement()?);
            if parser.at < parser.tokens.len() && !parser.eat(&Token::Semicolon) {
                return None;
            }
        }
        if stmts.is_empty() {
            return None;
        }
        let constant = match stmts.as_slice() {
            [Expr::Num(value)] => Some(*value),
            _ => None,
        };
        Some(Self { stmts, constant })
    }

    /// Runs the program; the result is the `return` value or the last statement's value.
    pub fn eval(&self, vars: &mut Vec<f32>, rng: &mut Rng, queries: &Queries) -> f32 {
        if let Some(value) = self.constant {
            return value;
        }
        let mut env = Env {
            vars,
            rng,
            queries,
            temps: [f32::NAN; MAX_TEMPS],
            returned: None,
        };
        let mut last = 0.0;
        for stmt in &self.stmts {
            last = env.exec(stmt);
            if let Some(value) = env.returned {
                return value;
            }
        }
        last
    }
}

struct Env<'a> {
    vars: &'a mut Vec<f32>,
    rng: &'a mut Rng,
    queries: &'a Queries,
    temps: [f32; MAX_TEMPS],
    returned: Option<f32>,
}

fn clean(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

impl Env<'_> {
    fn read_var(&self, slot: u16) -> f32 {
        self.vars
            .get(slot as usize)
            .copied()
            .filter(|value| !value.is_nan())
            .unwrap_or(0.0)
    }

    fn write_var(&mut self, slot: u16, value: f32) {
        let slot = slot as usize;
        if slot >= self.vars.len() {
            self.vars.resize(slot + 1, f32::NAN);
        }
        self.vars[slot] = value;
    }

    fn exec(&mut self, expr: &Expr) -> f32 {
        match expr {
            Expr::Num(value) => *value,
            Expr::Var(slot) => self.read_var(*slot),
            Expr::Temp(slot) => {
                let value = self.temps[*slot as usize];
                if value.is_nan() { 0.0 } else { value }
            }
            Expr::Query(query) => match query {
                Query::IsInWater => self.queries.is_in_water,
                Query::IsInWaterOrRain => self.queries.is_in_water_or_rain,
                Query::IsBaby => self.queries.is_baby,
                Query::GroundSpeed => self.queries.ground_speed,
                Query::FrameAlpha => self.queries.frame_alpha,
                Query::SpellColor(component) => self.queries.spell_color[*component],
                Query::Zero => 0.0,
            },
            Expr::Neg(inner) => -self.exec(inner),
            Expr::Not(inner) => f32::from(self.exec(inner) == 0.0),
            Expr::Bin(op, left, right) => {
                if *op == Op::And {
                    return f32::from(self.exec(left) != 0.0 && self.exec(right) != 0.0);
                }
                if *op == Op::Or {
                    return f32::from(self.exec(left) != 0.0 || self.exec(right) != 0.0);
                }
                let (a, b) = (self.exec(left), self.exec(right));
                clean(match op {
                    Op::Add => a + b,
                    Op::Sub => a - b,
                    Op::Mul => a * b,
                    Op::Div => {
                        if b == 0.0 {
                            0.0
                        } else {
                            a / b
                        }
                    }
                    Op::Lt => f32::from(a < b),
                    Op::Le => f32::from(a <= b),
                    Op::Gt => f32::from(a > b),
                    Op::Ge => f32::from(a >= b),
                    Op::Eq => f32::from(a == b),
                    Op::Ne => f32::from(a != b),
                    Op::And | Op::Or => 0.0,
                })
            }
            Expr::Cond(condition, yes, no) => {
                if self.exec(condition) != 0.0 {
                    self.exec(yes)
                } else {
                    self.exec(no)
                }
            }
            Expr::Coalesce(left, right) => {
                let unset = match left.as_ref() {
                    Expr::Var(slot) => self
                        .vars
                        .get(*slot as usize)
                        .is_none_or(|value| value.is_nan()),
                    Expr::Temp(slot) => self.temps[*slot as usize].is_nan(),
                    _ => false,
                };
                if unset {
                    self.exec(right)
                } else {
                    self.exec(left)
                }
            }
            Expr::Call(func, args) => {
                let mut values = [0.0f32; 4];
                for (slot, arg) in values.iter_mut().zip(args) {
                    *slot = self.exec(arg);
                }
                clean(self.call(*func, values))
            }
            Expr::AssignVar(slot, value) => {
                let value = self.exec(value);
                self.write_var(*slot, value);
                0.0
            }
            Expr::AssignTemp(slot, value) => {
                let value = self.exec(value);
                self.temps[*slot as usize] = value;
                0.0
            }
            Expr::Return(value) => {
                let value = self.exec(value);
                self.returned = Some(value);
                value
            }
        }
    }

    fn call(&mut self, func: Func, [a, b, c, _]: [f32; 4]) -> f32 {
        match func {
            Func::Abs => a.abs(),
            Func::Acos => a.clamp(-1.0, 1.0).acos().to_degrees(),
            Func::Asin => a.clamp(-1.0, 1.0).asin().to_degrees(),
            Func::Atan => a.atan().to_degrees(),
            Func::Atan2 => a.atan2(b).to_degrees(),
            Func::Ceil => a.ceil(),
            Func::Clamp => a.max(b).min(c.max(b)),
            Func::Cos => a.to_radians().cos(),
            Func::Exp => a.exp(),
            Func::Floor => a.floor(),
            Func::InverseLerp => {
                if a == b {
                    0.0
                } else {
                    (c - a) / (b - a)
                }
            }
            Func::Lerp => a + (b - a) * c,
            Func::LerpRotate => {
                let mut delta = (b - a).rem_euclid(360.0);
                if delta > 180.0 {
                    delta -= 360.0;
                }
                a + delta * c
            }
            Func::Ln => {
                if a > 0.0 {
                    a.ln()
                } else {
                    0.0
                }
            }
            Func::Max => a.max(b),
            Func::Min => a.min(b),
            Func::Mod => {
                if b == 0.0 {
                    0.0
                } else {
                    a % b
                }
            }
            Func::Pow => a.powf(b),
            Func::Random => self.rng.range(a, b),
            Func::RandomInteger => {
                let (low, high) = (a.min(b).ceil(), a.max(b).floor());
                (low + (self.rng.unit() * (high - low + 1.0)).floor()).min(high)
            }
            Func::DieRoll => {
                let mut total = 0.0;
                for _ in 0..(a.clamp(0.0, 64.0) as u32) {
                    total += self.rng.range(b, c);
                }
                total
            }
            Func::Round => a.round(),
            Func::Sign => {
                if a > 0.0 {
                    1.0
                } else if a < 0.0 {
                    -1.0
                } else {
                    0.0
                }
            }
            Func::Sin => a.to_radians().sin(),
            Func::Sqrt => {
                if a >= 0.0 {
                    a.sqrt()
                } else {
                    0.0
                }
            }
            Func::Trunc => a.trunc(),
            Func::HermiteBlend => {
                let t = a.clamp(0.0, 1.0);
                3.0 * t * t - 2.0 * t * t * t
            }
            Func::Zero => 0.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Num(f32),
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Semicolon,
    Question,
    Colon,
    Not,
    Lt,
    Le,
    Gt,
    Ge,
    EqEq,
    NotEq,
    Assign,
    AndAnd,
    OrOr,
    Coalesce,
}

fn lex(source: &str) -> Option<Vec<Token>> {
    let chars: Vec<char> = source.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        match c {
            ' ' | '\t' | '\n' | '\r' => {
                i += 1;
                continue;
            }
            '0'..='9' | '.' if c.is_ascii_digit() || next.is_some_and(|n| n.is_ascii_digit()) => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                    i += 1;
                }
                let text: String = chars[start..i].iter().collect();
                let value: f32 = text.parse().ok()?;
                if i < chars.len() && (chars[i] == 'f' || chars[i] == 'F') {
                    i += 1;
                }
                tokens.push(Token::Num(value));
                continue;
            }
            'a'..='z' | 'A'..='Z' | '_' => {
                let start = i;
                while i < chars.len()
                    && (chars[i].is_ascii_alphanumeric() || chars[i] == '_' || chars[i] == '.')
                {
                    i += 1;
                }
                tokens.push(Token::Ident(chars[start..i].iter().collect()));
                continue;
            }
            '\'' => {
                i += 1;
                while i < chars.len() && chars[i] != '\'' {
                    i += 1;
                }
                i += 1;
                tokens.push(Token::Num(0.0));
                continue;
            }
            '+' => tokens.push(Token::Plus),
            '-' => tokens.push(Token::Minus),
            '*' => tokens.push(Token::Star),
            '/' => tokens.push(Token::Slash),
            '(' => tokens.push(Token::LParen),
            ')' => tokens.push(Token::RParen),
            '[' => tokens.push(Token::LBracket),
            ']' => tokens.push(Token::RBracket),
            ',' => tokens.push(Token::Comma),
            ';' => tokens.push(Token::Semicolon),
            ':' => tokens.push(Token::Colon),
            '?' if next == Some('?') => {
                tokens.push(Token::Coalesce);
                i += 1;
            }
            '?' => tokens.push(Token::Question),
            '!' if next == Some('=') => {
                tokens.push(Token::NotEq);
                i += 1;
            }
            '!' => tokens.push(Token::Not),
            '<' if next == Some('=') => {
                tokens.push(Token::Le);
                i += 1;
            }
            '<' => tokens.push(Token::Lt),
            '>' if next == Some('=') => {
                tokens.push(Token::Ge);
                i += 1;
            }
            '>' => tokens.push(Token::Gt),
            '=' if next == Some('=') => {
                tokens.push(Token::EqEq);
                i += 1;
            }
            '=' => tokens.push(Token::Assign),
            '&' if next == Some('&') => {
                tokens.push(Token::AndAnd);
                i += 1;
            }
            '|' if next == Some('|') => {
                tokens.push(Token::OrOr);
                i += 1;
            }
            _ => return None,
        }
        i += 1;
    }
    Some(tokens)
}

struct Parser<'a> {
    tokens: Vec<Token>,
    at: usize,
    interner: &'a mut Interner,
    temps: Vec<String>,
    depth: u32,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }

    fn eat(&mut self, token: &Token) -> bool {
        if self.peek() == Some(token) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn temp_slot(&mut self, name: &str) -> Option<u8> {
        if let Some(index) = self.temps.iter().position(|known| known == name) {
            return Some(index as u8);
        }
        if self.temps.len() >= MAX_TEMPS {
            return None;
        }
        self.temps.push(name.to_owned());
        Some((self.temps.len() - 1) as u8)
    }

    fn statement(&mut self) -> Option<Expr> {
        if let Some(Token::Ident(name)) = self.peek() {
            let lower = name.to_ascii_lowercase();
            if lower == "return" {
                self.at += 1;
                return Some(Expr::Return(Box::new(self.expr()?)));
            }
            if self.tokens.get(self.at + 1) == Some(&Token::Assign) {
                self.at += 2;
                let value = Box::new(self.expr()?);
                if let Some(key) = variable_key(&lower) {
                    return Some(Expr::AssignVar(self.interner.intern(&key)?, value));
                }
                let key = lower
                    .strip_prefix("temp.")
                    .or_else(|| lower.strip_prefix("t."))?;
                return Some(Expr::AssignTemp(self.temp_slot(key)?, value));
            }
        }
        self.expr()
    }

    fn expr(&mut self) -> Option<Expr> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return None;
        }
        let result = self.coalesce();
        self.depth -= 1;
        result
    }

    fn coalesce(&mut self) -> Option<Expr> {
        let left = self.ternary()?;
        if self.eat(&Token::Coalesce) {
            let right = self.ternary()?;
            return Some(Expr::Coalesce(Box::new(left), Box::new(right)));
        }
        Some(left)
    }

    fn ternary(&mut self) -> Option<Expr> {
        let condition = self.or()?;
        if self.eat(&Token::Question) {
            let yes = self.expr()?;
            let no = if self.eat(&Token::Colon) {
                self.expr()?
            } else {
                Expr::Num(0.0)
            };
            return Some(Expr::Cond(Box::new(condition), Box::new(yes), Box::new(no)));
        }
        Some(condition)
    }

    fn binary_level(
        &mut self,
        next: fn(&mut Self) -> Option<Expr>,
        ops: &[(Token, Op)],
    ) -> Option<Expr> {
        let mut left = next(self)?;
        'outer: loop {
            for (token, op) in ops {
                if self.peek() == Some(token) {
                    self.at += 1;
                    let right = next(self)?;
                    left = Expr::Bin(*op, Box::new(left), Box::new(right));
                    continue 'outer;
                }
            }
            return Some(left);
        }
    }

    fn or(&mut self) -> Option<Expr> {
        self.binary_level(Self::and, &[(Token::OrOr, Op::Or)])
    }

    fn and(&mut self) -> Option<Expr> {
        self.binary_level(Self::equality, &[(Token::AndAnd, Op::And)])
    }

    fn equality(&mut self) -> Option<Expr> {
        self.binary_level(
            Self::relational,
            &[(Token::EqEq, Op::Eq), (Token::NotEq, Op::Ne)],
        )
    }

    fn relational(&mut self) -> Option<Expr> {
        self.binary_level(
            Self::additive,
            &[
                (Token::Lt, Op::Lt),
                (Token::Le, Op::Le),
                (Token::Gt, Op::Gt),
                (Token::Ge, Op::Ge),
            ],
        )
    }

    fn additive(&mut self) -> Option<Expr> {
        self.binary_level(
            Self::multiplicative,
            &[(Token::Plus, Op::Add), (Token::Minus, Op::Sub)],
        )
    }

    fn multiplicative(&mut self) -> Option<Expr> {
        self.binary_level(
            Self::unary,
            &[(Token::Star, Op::Mul), (Token::Slash, Op::Div)],
        )
    }

    /// Counts unary prefixes against the same nesting budget as subexpressions.
    fn unary(&mut self) -> Option<Expr> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return None;
        }
        let result = self.unary_inner();
        self.depth -= 1;
        result
    }

    /// Parses one unary operator or primary within the guarded recursion.
    fn unary_inner(&mut self) -> Option<Expr> {
        if self.eat(&Token::Minus) {
            return Some(match self.unary()? {
                Expr::Num(value) => Expr::Num(-value),
                other => Expr::Neg(Box::new(other)),
            });
        }
        if self.eat(&Token::Plus) {
            return self.unary();
        }
        if self.eat(&Token::Not) {
            return Some(Expr::Not(Box::new(self.unary()?)));
        }
        self.primary()
    }

    fn primary(&mut self) -> Option<Expr> {
        match self.tokens.get(self.at)?.clone() {
            Token::Num(value) => {
                self.at += 1;
                Some(Expr::Num(value))
            }
            Token::LParen => {
                self.at += 1;
                let inner = self.expr()?;
                self.eat(&Token::RParen).then_some(inner)
            }
            Token::Ident(name) => {
                self.at += 1;
                let lower = name.to_ascii_lowercase();
                if self.eat(&Token::LParen) {
                    let mut args = Vec::new();
                    if !self.eat(&Token::RParen) {
                        loop {
                            args.push(self.expr()?);
                            if self.eat(&Token::RParen) {
                                break;
                            }
                            if !self.eat(&Token::Comma) {
                                return None;
                            }
                        }
                    }
                    return Some(Self::fold_call(function(&lower), args));
                }
                let expr = self.reference(&lower)?;
                if self.eat(&Token::LBracket) {
                    // Array indexing has no producer in particle context.
                    self.expr()?;
                    self.eat(&Token::RBracket).then_some(())?;
                    return Some(Expr::Num(0.0));
                }
                Some(expr)
            }
            _ => None,
        }
    }

    fn reference(&mut self, lower: &str) -> Option<Expr> {
        if let Some(key) = variable_key(lower) {
            return Some(Expr::Var(self.interner.intern(&key)?));
        }
        if let Some(key) = lower
            .strip_prefix("temp.")
            .or_else(|| lower.strip_prefix("t."))
        {
            return Some(Expr::Temp(self.temp_slot(key)?));
        }
        if let Some(key) = lower
            .strip_prefix("query.")
            .or_else(|| lower.strip_prefix("q."))
        {
            return Some(Expr::Query(query(key)));
        }
        if lower == "math.pi" {
            return Some(Expr::Num(std::f32::consts::PI));
        }
        if lower == "true" {
            return Some(Expr::Num(1.0));
        }
        if lower == "false" {
            return Some(Expr::Num(0.0));
        }
        Some(Expr::Num(0.0))
    }

    fn fold_call(func: Func, args: Vec<Expr>) -> Expr {
        Expr::Call(func, args)
    }
}

fn query(name: &str) -> Query {
    match name {
        "is_in_water" => Query::IsInWater,
        "is_in_water_or_rain" => Query::IsInWaterOrRain,
        "is_baby" => Query::IsBaby,
        "ground_speed" => Query::GroundSpeed,
        "frame_alpha" => Query::FrameAlpha,
        "spellcolor.r" | "spellcolor.red" => Query::SpellColor(0),
        "spellcolor.g" | "spellcolor.green" => Query::SpellColor(1),
        "spellcolor.b" | "spellcolor.blue" => Query::SpellColor(2),
        "spellcolor.a" | "spellcolor.alpha" => Query::SpellColor(3),
        _ => Query::Zero,
    }
}

fn function(name: &str) -> Func {
    match name.strip_prefix("math.").unwrap_or(name) {
        "abs" => Func::Abs,
        "acos" => Func::Acos,
        "asin" => Func::Asin,
        "atan" => Func::Atan,
        "atan2" => Func::Atan2,
        "ceil" => Func::Ceil,
        "clamp" => Func::Clamp,
        "cos" => Func::Cos,
        "exp" => Func::Exp,
        "floor" => Func::Floor,
        "inverse_lerp" => Func::InverseLerp,
        "lerp" => Func::Lerp,
        "lerprotate" => Func::LerpRotate,
        "ln" => Func::Ln,
        "max" => Func::Max,
        "min" => Func::Min,
        "mod" => Func::Mod,
        "pow" => Func::Pow,
        "random" => Func::Random,
        "random_integer" => Func::RandomInteger,
        "die_roll" | "die_roll_integer" => Func::DieRoll,
        "round" => Func::Round,
        "sign" => Func::Sign,
        "sin" => Func::Sin,
        "sqrt" => Func::Sqrt,
        "trunc" => Func::Trunc,
        "hermite_blend" => Func::HermiteBlend,
        _ => Func::Zero,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(source: &str) -> f32 {
        let mut interner = Interner::with_builtins();
        let program = Program::parse(source, &mut interner).expect("parses");
        let mut vars = vec![f32::NAN; interner.len()];
        program.eval(&mut vars, &mut Rng::new(1), &Queries::default())
    }

    #[test]
    fn evaluates_arithmetic_precedence_and_float_suffix() {
        assert_eq!(run("1 + 2 * 3"), 7.0);
        assert_eq!(run("0.5f * 4"), 2.0);
        assert_eq!(run("-2 * -3"), 6.0);
    }

    #[test]
    fn ternary_comparison_and_logic() {
        assert_eq!(run("(2 > 1) ? 10 : 20"), 10.0);
        assert_eq!(run("1 && 0 || 1"), 1.0);
        assert_eq!(run("!0"), 1.0);
    }

    #[test]
    fn math_functions_are_case_insensitive_and_use_degrees() {
        assert!((run("Math.Sin(90)") - 1.0).abs() < 1e-5);
        assert_eq!(run("math.clamp(5, 0, 3)"), 3.0);
        assert_eq!(run("math.lerp(0, 10, 0.5)"), 5.0);
        assert_eq!(run("math.pow(2, 3)"), 8.0);
    }

    #[test]
    fn scripts_assign_variables_and_return() {
        assert_eq!(run("v.a = 3; v.b = v.a * 2; return v.b + 1;"), 7.0);
        assert_eq!(run("temp.x = 4; return temp.x;"), 4.0);
    }

    #[test]
    fn coalesce_uses_the_fallback_only_for_unassigned_variables() {
        assert_eq!(run("v.unset ?? 5"), 5.0);
        assert_eq!(run("v.set = 0; v.set ?? 5"), 0.0);
    }

    #[test]
    fn division_by_zero_and_unknown_names_read_zero() {
        assert_eq!(run("1 / 0"), 0.0);
        assert_eq!(run("query.nothing + 2"), 2.0);
    }

    #[test]
    fn random_is_deterministic_per_seed_and_in_range() {
        let a = run("math.random(2, 4)");
        assert_eq!(a, run("math.random(2, 4)"));
        assert!((2.0..4.0).contains(&a));
    }

    #[test]
    fn rejects_garbage() {
        let mut interner = Interner::with_builtins();
        assert!(Program::parse("1 +", &mut interner).is_none());
        assert!(Program::parse("@", &mut interner).is_none());
    }
}

#[cfg(test)]
#[path = "molang_limits_tests.rs"]
mod limits_tests;
