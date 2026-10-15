//! Molang subset particle definitions use: arithmetic, ternaries, `math.*`, `variable.*`,
//! `temp.*`, `query.*`, `return` and assignments. Variables are interned per effect so a
//! particle's state is a dense `Vec<f32>`; a NaN slot is "unassigned" and reads as 0.

use std::{collections::BTreeMap, sync::Arc};
mod bytecode;
mod interner;
mod query_support;
pub use interner::Interner;

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
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Queries {
    pub is_in_water: f32,
    pub is_in_water_or_rain: f32,
    pub is_baby: f32,
    pub ground_speed: f32,
    pub frame_alpha: f32,
    /// Potion swirl colour, linear 0..1 RGBA.
    pub spell_color: [f32; 4],
    /// Named actor queries supplied by the host for a bound controller emitter.
    pub actor: Option<Arc<BTreeMap<Box<str>, f32>>>,
}

#[derive(Clone, Debug)]
enum Query {
    IsInWater,
    IsInWaterOrRain,
    IsBaby,
    GroundSpeed,
    FrameAlpha,
    SpellColor(usize),
    Named(Box<str>),
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
    Call(Option<assets::entity::MolangFunction>, Vec<Expr>),
    AssignVar(u16, Box<Expr>),
    AssignTemp(u8, Box<Expr>),
    Return(Box<Expr>),
}

/// A parsed expression or `;`-separated script, compiled to flat code.
#[derive(Clone, Debug)]
pub struct Program {
    code: bytecode::Code,
    constant: Option<f32>,
}

const MAX_TEMPS: usize = 8;
const MAX_DEPTH: u32 = 48;
// Bounds flat AST chains too, including recursive compilation and destruction.
const MAX_TOKENS: usize = 256;

impl Program {
    #[must_use]
    pub fn constant(value: f32) -> Self {
        Self::compile(&[Expr::Num(value)])
    }

    /// Parses `source`; unparseable text yields `None` (vanilla evaluates it as 0).
    #[must_use]
    pub fn parse(source: &str, interner: &mut Interner) -> Option<Self> {
        parse_statements(source, interner).map(|statements| Self::compile(&statements))
    }

    /// Lowers parsed statements; a lone number short-circuits evaluation.
    fn compile(statements: &[Expr]) -> Self {
        let constant = match statements {
            [Expr::Num(value)] => Some(*value),
            _ => None,
        };
        Self {
            code: bytecode::compile(statements),
            constant,
        }
    }

    /// Runs the program; the result is the `return` value or the last statement's value.
    pub fn eval(&self, vars: &mut Vec<f32>, rng: &mut Rng, queries: &Queries) -> f32 {
        if let Some(value) = self.constant {
            return value;
        }
        self.code.eval(vars, rng, queries)
    }
}

/// Parses `source` into its non-empty statement list; `None` when it does not parse.
fn parse_statements(source: &str, interner: &mut Interner) -> Option<Vec<Expr>> {
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
    Some(stmts)
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

    fn fold_call(func: Option<assets::entity::MolangFunction>, args: Vec<Expr>) -> Expr {
        Expr::Call(func, args)
    }
}

/// Resolves built-in queries and retains other numeric actor query names.
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
        _ => Query::Named(name.into()),
    }
}

/// Resolves math spellings through the shared function vocabulary.
fn function(name: &str) -> Option<assets::entity::MolangFunction> {
    assets::entity::MolangFunction::from_name(name)
        .or_else(|| assets::entity::MolangFunction::from_name(&format!("math.{name}")))
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
    fn particle_math_uses_the_shared_vocabulary_and_integer_die_rolls() {
        assert_eq!(run("math.copy_sign(2, -1)"), -2.0);
        assert_eq!(run("math.min_angle(450)"), 90.0);
        assert_eq!(run("math.ease_in_quad(0, 8, 0.5)"), 2.0);
        for count in 1..=8 {
            let value = run(&format!("math.die_roll_integer({count}, 1, 6)"));
            assert_eq!(value.fract(), 0.0);
            assert!((count as f32..=count as f32 * 6.0).contains(&value));
        }
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
    fn skipped_operands_draw_no_randoms() {
        let next = run("math.random(0, 1)");
        for source in [
            "0 && math.random(0, 1); math.random(0, 1)",
            "1 || math.random(0, 1); math.random(0, 1)",
            "1 ? 2 : math.random(0, 1); math.random(0, 1)",
            "0 ? math.random(0, 1) : 2; math.random(0, 1)",
            "v.set = 1; v.set ?? math.random(0, 1); math.random(0, 1)",
            "2 ?? math.random(0, 1); math.random(0, 1)",
            "math.max(1, 2, 3, 4, math.random(0, 1)); math.random(0, 1)",
        ] {
            assert_eq!(run(source), next, "{source}");
        }
    }

    #[test]
    fn return_stops_later_statements() {
        let mut interner = Interner::with_builtins();
        let program = Program::parse("v.a = 1; return 2; v.a = 3", &mut interner).unwrap();
        let mut vars = vec![f32::NAN; interner.len()];
        let value = program.eval(&mut vars, &mut Rng::new(1), &Queries::default());
        assert_eq!(value, 2.0);
        assert_eq!(vars[interner.get("a").unwrap() as usize], 1.0);
    }

    #[test]
    fn deeply_pending_operands_evaluate() {
        let source = format!("{}1{}", "1==1<1+1*(".repeat(10), ")".repeat(10));
        assert_eq!(run(&source), 1.0);
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
