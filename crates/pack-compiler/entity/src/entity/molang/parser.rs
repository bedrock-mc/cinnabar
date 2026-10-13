use assets::{AssetError, MolangFunction};

use super::lexer::{Token, tokenize};
use crate::entity::invalid;

const MAX_PARSE_DEPTH: usize = 48;
const MAX_STATEMENTS: usize = 256;

/// Where a named value lives.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum Slot {
    Variable,
    Temporary,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Unary {
    Negate,
    Not,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Binary {
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
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Expr {
    Number(f32),
    String(Box<str>),
    This,
    Variable(Slot, Box<str>),
    Query(Box<str>, Option<Vec<Expr>>),
    Call(MolangFunction, Vec<Expr>),
    Unary(Unary, Box<Expr>),
    Binary(Binary, Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    /// `condition ? yes : no`, or `condition ? yes` which reads 0.0 when false.
    Conditional(Box<Expr>, Box<Expr>, Option<Box<Expr>>),
    Coalesce(Slot, Box<str>, Box<Expr>),
    Arrow(Box<Expr>, Box<Expr>),
    /// `condition ? { ... } : { ... }` or `condition ? break`: statements, no value.
    Branch(Box<Expr>, Vec<Statement>, Option<Vec<Statement>>),
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Statement {
    Expression(Expr),
    Assign(Slot, Box<str>, Expr),
    Return(Option<Expr>),
    Break,
    Continue,
    Loop(Expr, Vec<Statement>),
    ForEach(Slot, Box<str>, Expr, Vec<Statement>),
    Block(Vec<Statement>),
}

/// A simple expression yields its value; a complex one yields 0.0 unless it returns.
#[derive(Debug, PartialEq)]
pub(super) enum Program {
    Simple(Expr),
    Complex(Vec<Statement>),
}

pub(super) fn parse(source: &str) -> Result<Program, AssetError> {
    parse_with(source, assets::MOLANG_QUERIES)
}

/// Parses with only the sorted `queries` admitted, as a context restricts what it can read.
pub(super) fn parse_with(source: &str, queries: &[&str]) -> Result<Program, AssetError> {
    let tokens = tokenize(source)?;
    let complex = tokens
        .iter()
        .any(|token| matches!(token, Token::Semicolon | Token::Assign));
    let mut parser = Parser {
        tokens,
        cursor: 0,
        queries,
    };
    if !complex {
        let expression = parser.expression(0)?;
        parser.expect(&Token::End)?;
        return Ok(Program::Simple(expression));
    }
    let statements = parser.statements(&Token::End)?;
    // Vanilla rejects a complex expression whose last statement lacks its terminator.
    if parser.tokens.get(parser.tokens.len().wrapping_sub(2)) != Some(&Token::Semicolon) {
        return Err(invalid("complex Molang expression must end with `;`"));
    }
    Ok(Program::Complex(statements))
}

struct Parser<'q> {
    tokens: Vec<Token>,
    cursor: usize,
    queries: &'q [&'q str],
}

impl Parser<'_> {
    fn peek(&self) -> &Token {
        &self.tokens[self.cursor]
    }

    fn bump(&mut self) -> Token {
        let token = self.tokens[self.cursor].clone();
        if token != Token::End {
            self.cursor += 1;
        }
        token
    }

    fn eat(&mut self, token: &Token) -> bool {
        let found = self.peek() == token;
        if found {
            self.bump();
        }
        found
    }

    fn expect(&mut self, token: &Token) -> Result<(), AssetError> {
        if self.eat(token) {
            Ok(())
        } else {
            Err(invalid("unexpected Molang token"))
        }
    }

    fn statements(&mut self, close: &Token) -> Result<Vec<Statement>, AssetError> {
        let mut statements = Vec::new();
        let mut returned = false;
        while self.peek() != close {
            if statements.len() == MAX_STATEMENTS {
                return Err(invalid("Molang statement count exceeds bound"));
            }
            if returned {
                return Err(invalid("Molang statement follows a return"));
            }
            let statement = self.statement(0)?;
            returned = matches!(statement, Statement::Return(_));
            statements.push(statement);
            if !self.eat(&Token::Semicolon) && self.peek() != close {
                return Err(invalid("Molang statements must be `;` separated"));
            }
        }
        Ok(statements)
    }

    fn block(&mut self, depth: usize) -> Result<Vec<Statement>, AssetError> {
        check_depth(depth)?;
        self.expect(&Token::LeftBrace)?;
        let statements = self.statements(&Token::RightBrace)?;
        self.expect(&Token::RightBrace)?;
        Ok(statements)
    }

    fn statement(&mut self, depth: usize) -> Result<Statement, AssetError> {
        check_depth(depth)?;
        if let Token::Identifier(keyword) = self.peek() {
            match keyword.as_ref() {
                "return" => {
                    self.bump();
                    return Ok(Statement::Return(
                        (!matches!(
                            self.peek(),
                            Token::Semicolon | Token::RightBrace | Token::End
                        ))
                        .then(|| self.expression(depth + 1))
                        .transpose()?,
                    ));
                }
                "break" => {
                    self.bump();
                    return Ok(Statement::Break);
                }
                "continue" => {
                    self.bump();
                    return Ok(Statement::Continue);
                }
                "loop" => {
                    self.bump();
                    self.expect(&Token::LeftParen)?;
                    let count = self.expression(depth + 1)?;
                    self.expect(&Token::Comma)?;
                    let body = self.block(depth + 1)?;
                    self.expect(&Token::RightParen)?;
                    return Ok(Statement::Loop(count, body));
                }
                "for_each" => {
                    self.bump();
                    self.expect(&Token::LeftParen)?;
                    let Expr::Variable(slot, name) = self.primary(depth + 1)? else {
                        return Err(invalid("for_each needs a variable"));
                    };
                    self.expect(&Token::Comma)?;
                    let array = self.expression(depth + 1)?;
                    self.expect(&Token::Comma)?;
                    let body = self.block(depth + 1)?;
                    self.expect(&Token::RightParen)?;
                    return Ok(Statement::ForEach(slot, name, array, body));
                }
                _ => {}
            }
        }
        if self.peek() == &Token::LeftBrace {
            return Ok(Statement::Block(self.block(depth + 1)?));
        }
        let target = self.expression(depth + 1)?;
        if !self.eat(&Token::Assign) {
            return Ok(Statement::Expression(target));
        }
        let Expr::Variable(slot, name) = target else {
            return Err(invalid("Molang assignment target must be a variable"));
        };
        Ok(Statement::Assign(slot, name, self.expression(depth + 1)?))
    }

    /// Precedence levels from loosest: `??`, `?:`, `||`, `&&`, equality, relational,
    /// additive, `*`, `/`, unary, postfix.
    fn expression(&mut self, depth: usize) -> Result<Expr, AssetError> {
        check_depth(depth)?;
        let left = self.conditional(depth)?;
        if self.peek() != &Token::Operator("??") {
            return Ok(left);
        }
        self.bump();
        let Expr::Variable(slot, name) = left else {
            return Err(invalid("Molang `??` requires a variable"));
        };
        Ok(Expr::Coalesce(
            slot,
            name,
            Box::new(self.expression(depth + 1)?),
        ))
    }

    fn conditional(&mut self, depth: usize) -> Result<Expr, AssetError> {
        let condition = self.binary(0, depth)?;
        if !self.eat(&Token::Question) {
            return Ok(condition);
        }
        if self.statement_branch_follows() {
            let yes = self.branch_body(depth + 1)?;
            let no = if self.eat(&Token::Colon) {
                Some(self.branch_body(depth + 1)?)
            } else {
                None
            };
            return Ok(Expr::Branch(Box::new(condition), yes, no));
        }
        let yes = self.conditional(depth + 1)?;
        let no = if self.eat(&Token::Colon) {
            Some(Box::new(self.conditional(depth + 1)?))
        } else {
            None
        };
        Ok(Expr::Conditional(Box::new(condition), Box::new(yes), no))
    }

    fn statement_branch_follows(&self) -> bool {
        match self.peek() {
            Token::LeftBrace => true,
            Token::Identifier(keyword) => {
                matches!(keyword.as_ref(), "break" | "continue" | "return")
            }
            _ => false,
        }
    }

    fn branch_body(&mut self, depth: usize) -> Result<Vec<Statement>, AssetError> {
        if self.peek() == &Token::LeftBrace {
            self.block(depth)
        } else {
            Ok(vec![self.statement(depth)?])
        }
    }

    /// Precedence levels share one nesting depth; only nested operands deepen it.
    fn binary(&mut self, level: usize, depth: usize) -> Result<Expr, AssetError> {
        const LEVELS: [&[&str]; 7] = [
            &["||"],
            &["&&"],
            &["==", "!="],
            &["<", "<=", ">", ">="],
            &["+", "-"],
            &["*"],
            &["/"],
        ];
        if level == LEVELS.len() {
            return self.unary(depth);
        }
        let mut left = self.binary(level + 1, depth)?;
        while let Token::Operator(operator) = *self.peek() {
            if !LEVELS[level].contains(&operator) {
                break;
            }
            self.bump();
            let right = Box::new(self.binary(level + 1, depth)?);
            let left_box = Box::new(left);
            left = match operator {
                "||" => Expr::Or(left_box, right),
                "&&" => Expr::And(left_box, right),
                _ => Expr::Binary(binary_operator(operator), left_box, right),
            };
        }
        Ok(left)
    }

    fn unary(&mut self, depth: usize) -> Result<Expr, AssetError> {
        check_depth(depth)?;
        let operator = match self.peek() {
            Token::Operator("-") => Unary::Negate,
            Token::Operator("!") => Unary::Not,
            _ => return self.postfix(depth),
        };
        self.bump();
        Ok(Expr::Unary(operator, Box::new(self.unary(depth + 1)?)))
    }

    fn postfix(&mut self, depth: usize) -> Result<Expr, AssetError> {
        let left = self.primary(depth)?;
        if !self.eat(&Token::Arrow) {
            return Ok(left);
        }
        let right = self.primary(depth + 1)?;
        if self.peek() == &Token::Arrow {
            return Err(invalid(
                "nested Molang `->` access is unsupported by vanilla",
            ));
        }
        Ok(Expr::Arrow(Box::new(left), Box::new(right)))
    }

    fn arguments(&mut self, depth: usize) -> Result<Vec<Expr>, AssetError> {
        self.expect(&Token::LeftParen)?;
        let mut arguments = Vec::new();
        if !self.eat(&Token::RightParen) {
            loop {
                arguments.push(self.expression(depth + 1)?);
                if self.eat(&Token::RightParen) {
                    break;
                }
                self.expect(&Token::Comma)?;
            }
        }
        Ok(arguments)
    }

    fn primary(&mut self, depth: usize) -> Result<Expr, AssetError> {
        check_depth(depth)?;
        match self.bump() {
            Token::Number(value) => Ok(Expr::Number(value)),
            Token::String(text) => Ok(Expr::String(text)),
            Token::LeftParen => {
                let expression = self.expression(depth + 1)?;
                self.expect(&Token::RightParen)?;
                Ok(expression)
            }
            Token::Identifier(name) => self.name(name, depth),
            _ => Err(invalid("expected a Molang value")),
        }
    }

    fn name(&mut self, name: Box<str>, depth: usize) -> Result<Expr, AssetError> {
        let calls = self.peek() == &Token::LeftParen;
        match name.as_ref() {
            "true" if !calls => return Ok(Expr::Number(1.0)),
            "false" if !calls => return Ok(Expr::Number(0.0)),
            "this" if !calls => return Ok(Expr::This),
            "math.pi" if !calls => return Ok(Expr::Number(std::f32::consts::PI)),
            _ => {}
        }
        if name.starts_with("math.") {
            let function = MolangFunction::from_name(&name)
                .ok_or_else(|| invalid("unknown Molang function"))?;
            let arguments = self.arguments(depth)?;
            if arguments.len() != function.arity() {
                return Err(invalid("Molang function has invalid arity"));
            }
            return Ok(Expr::Call(function, arguments));
        }
        if name.starts_with("query.") {
            if self.queries.binary_search(&name.as_ref()).is_err() {
                return Err(invalid("unknown Molang query"));
            }
            let arguments = calls.then(|| self.arguments(depth)).transpose()?;
            return Ok(Expr::Query(name, arguments));
        }
        let slot = if name.starts_with("variable.") || name.starts_with("context.") {
            Slot::Variable
        } else if name.starts_with("temp.") {
            Slot::Temporary
        } else if ["geometry.", "texture.", "material."]
            .iter()
            .any(|prefix| name.starts_with(prefix))
        {
            // Render-controller resource references evaluate to their resource name.
            return Ok(Expr::String(name));
        } else {
            return Err(invalid("unknown Molang identifier"));
        };
        if calls || !valid_path(&name) {
            return Err(invalid("invalid Molang variable"));
        }
        Ok(Expr::Variable(slot, name))
    }
}

fn valid_path(name: &str) -> bool {
    name.split('.').skip(1).all(|segment| {
        !segment.is_empty()
            && segment
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    }) && name.split('.').count() > 1
}

fn binary_operator(operator: &str) -> Binary {
    match operator {
        "==" => Binary::Equal,
        "!=" => Binary::NotEqual,
        "<" => Binary::Less,
        "<=" => Binary::LessEqual,
        ">" => Binary::Greater,
        ">=" => Binary::GreaterEqual,
        "+" => Binary::Add,
        "-" => Binary::Subtract,
        "*" => Binary::Multiply,
        _ => Binary::Divide,
    }
}

fn check_depth(depth: usize) -> Result<(), AssetError> {
    if depth > MAX_PARSE_DEPTH {
        Err(invalid("Molang parse depth exceeds bound"))
    } else {
        Ok(())
    }
}
