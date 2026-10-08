//! Expression evaluation: a value/operator stack where prefix `+`, `-` and
//! `not` reduce as soon as their operand arrives and binary operators reduce
//! left to right by precedence, with the client's per-type operator rules.

use super::token::{
    AND, DIVIDE, EQUAL, GREATER, LESS, MINUS, NOT, OR, Operand, PLUS, TIMES, Token, reparse,
};

/// A stack entry: a value, or an operator waiting for its right operand.
#[derive(Clone, Debug)]
pub(super) enum Item {
    Value(Operand),
    Operator(u8),
}

impl Item {
    fn op(&self) -> u8 {
        match self {
            Item::Operator(op) => *op,
            Item::Value(Operand::OperatorText(op)) => *op,
            Item::Value(_) => 0,
        }
    }

    fn is_operator(&self) -> bool {
        matches!(
            self,
            Item::Operator(_) | Item::Value(Operand::OperatorText(_))
        )
    }

    fn operand(&self) -> Operand {
        match self {
            Item::Value(operand) => operand.clone(),
            Item::Operator(op) => Operand::OperatorText(*op),
        }
    }
}

/// Resolves the tokens that read outside the expression.
pub(super) trait Scope {
    /// `#name` from the bag, `None` without one (undecidable).
    fn property(&self, name: &str) -> Option<Operand>;
    /// A `#`-text result's property; without a bag, the name as text.
    fn result_property(&self, name: &str) -> Option<Operand>;
    fn variable(&self, name: &str) -> Option<Operand>;
}

/// `and`/`or` 1, comparisons 2, `+`/`-` 3, `*`/`/` 4; `not` and nothing 0.
fn precedence(op: u8) -> u8 {
    match op {
        AND | OR => 1,
        GREATER | LESS | EQUAL => 2,
        PLUS | MINUS => 3,
        TIMES | DIVIDE => 4,
        _ => 0,
    }
}

fn unary(op: u8) -> bool {
    matches!(op, PLUS | MINUS | NOT)
}

/// Evaluate a token list to its final operand (properties still unresolved),
/// or `None` when a property has no bag to read; an ill-formed list is null.
pub(super) fn evaluate(tokens: &[Token], scope: &dyn Scope) -> Option<Operand> {
    let mut stack: Vec<Item> = Vec::with_capacity(tokens.len());
    for (index, token) in tokens.iter().enumerate() {
        let item = match token {
            Token::Group(inner) => {
                let result = resolve_final(evaluate(inner, scope)?, scope)?;
                Item::Value(Operand::from_json(&to_json(result)))
            }
            Token::Property(name) => Item::Value(scope.property(name)?),
            Token::Variable(name) => Item::Value(scope.variable(name)?),
            Token::Value(operand) => Item::Value(operand.clone()),
            Token::Operator(op) => Item::Operator(*op),
        };
        let pushed_operator = item.is_operator();
        stack.push(item);
        if pushed_operator {
            continue;
        }
        reduce_unary(&mut stack);
        let next = match tokens.get(index + 1) {
            Some(Token::Operator(op)) => *op,
            _ => 0,
        };
        if stack.len() > 1 {
            let top = stack[stack.len() - 2].op();
            if top != 0 && precedence(next) <= precedence(top) {
                reduce_binary(next, &mut stack);
            }
        }
    }
    match stack.as_slice() {
        [only] => Some(only.operand()),
        _ => Some(Operand::Other(serde_json::Value::Null)),
    }
}

/// The JSON an evaluated operand yields: untyped results read as ints.
pub(super) fn to_json(operand: Operand) -> serde_json::Value {
    use serde_json::Value;
    match operand {
        Operand::Bool(flag) => Value::Bool(flag),
        Operand::Float(value) => {
            serde_json::Number::from_f64(f64::from(value)).map_or(Value::Null, Value::Number)
        }
        Operand::Str(text) => Value::String(text),
        Operand::Other(value) => value,
        other => Value::from(other.int()),
    }
}

/// A final `#`-text result reads that property, as the client's last step does.
pub(super) fn resolve_final(operand: Operand, scope: &dyn Scope) -> Option<Operand> {
    match operand {
        Operand::PropertyText(name) => scope.result_property(&name),
        other => Some(other),
    }
}

fn reduce_unary(stack: &mut Vec<Item>) {
    while stack.len() >= 2 {
        let len = stack.len();
        let op = stack[len - 2].op();
        let leads = len == 2 || stack[len - 3].is_operator();
        if !unary(op) || !stack[len - 2].is_operator() || !leads {
            break;
        }
        let value = stack
            .pop()
            .map_or(Operand::Other(serde_json::Value::Null), |item| {
                item.operand()
            });
        stack.pop();
        stack.push(Item::Value(match op {
            PLUS if value.is_float() => Operand::Float(value.float()),
            PLUS => Operand::Int(value.int()),
            MINUS if value.is_float() => Operand::Float(-value.float()),
            MINUS => Operand::Int(value.int().wrapping_neg()),
            _ => Operand::Bool(!value.truthy()),
        }));
    }
}

fn reduce_binary(next: u8, stack: &mut Vec<Item>) {
    let floor = precedence(next);
    while stack.len() > 2 {
        let len = stack.len();
        let top = &stack[len - 2];
        if precedence(top.op()) < floor || !top.is_operator() {
            break;
        }
        let op = top.op();
        let rhs = stack[len - 1].operand();
        let lhs = stack[len - 3].operand();
        stack.truncate(len - 3);
        stack.push(Item::Value(apply(op, lhs, rhs)));
    }
}

/// One binary operator with the client's type rules.
pub(super) fn apply(op: u8, lhs: Operand, rhs: Operand) -> Operand {
    match op {
        AND => Operand::Bool(lhs.truthy() && rhs.truthy()),
        OR => Operand::Bool(lhs.truthy() || rhs.truthy()),
        GREATER | LESS => Operand::Bool(order(op, &lhs, &rhs)),
        EQUAL => Operand::Bool(equal(&lhs, &rhs)),
        PLUS => plus(lhs, rhs),
        MINUS => minus(lhs, rhs),
        TIMES => times(lhs, rhs),
        DIVIDE => divide(lhs, rhs),
        _ => Operand::Other(serde_json::Value::Null),
    }
}

/// `>`/`<`: strings by bytes, one string by truthiness, else as floats.
fn order(op: u8, lhs: &Operand, rhs: &Operand) -> bool {
    let greater = op == GREATER;
    match (lhs.is_string(), rhs.is_string()) {
        (true, true) => {
            let ordering = lhs.text().as_bytes().cmp(rhs.text().as_bytes());
            if greater {
                ordering.is_gt()
            } else {
                ordering.is_lt()
            }
        }
        (true, false) | (false, true) => {
            if greater {
                lhs.truthy() && !rhs.truthy()
            } else {
                !lhs.truthy() && rhs.truthy()
            }
        }
        (false, false) => {
            let (a, b) = (lhs.float(), rhs.float());
            if greater { a > b } else { a < b }
        }
    }
}

/// `=`: strings by bytes, one string only when both are falsy, bools by
/// truthiness, else as floats.
fn equal(lhs: &Operand, rhs: &Operand) -> bool {
    match (lhs.is_string(), rhs.is_string()) {
        (true, true) => lhs.text() == rhs.text(),
        (true, false) | (false, true) => !lhs.truthy() && !rhs.truthy(),
        (false, false) if lhs.is_bool() || rhs.is_bool() => lhs.truthy() == rhs.truthy(),
        (false, false) => lhs.float() == rhs.float(),
    }
}

fn numeric(
    lhs: &Operand,
    rhs: &Operand,
    int: fn(i32, i32) -> i32,
    float: fn(f32, f32) -> f32,
) -> Operand {
    if lhs.is_float() || rhs.is_float() {
        Operand::Float(float(lhs.float(), rhs.float()))
    } else {
        Operand::Int(int(lhs.int(), rhs.int()))
    }
}

/// `+`: text concatenation (an int side as decimal) or numeric addition.
fn plus(lhs: Operand, rhs: Operand) -> Operand {
    let text = |operand: &Operand| match operand {
        Operand::Int(value) => value.to_string(),
        other => other.text().to_owned(),
    };
    if lhs.is_string() {
        let right = match rhs {
            Operand::Int(value) => value.to_string(),
            ref other => other.text().to_owned(),
        };
        return reparse(format!("{}{right}", lhs.text()));
    }
    if rhs.is_string() {
        return reparse(format!("{}{}", text(&lhs), rhs.text()));
    }
    numeric(&lhs, &rhs, i32::wrapping_add, |a, b| a + b)
}

/// `-`: text removes every non-overlapping match, else numeric subtraction.
fn minus(lhs: Operand, rhs: Operand) -> Operand {
    if lhs.is_string() {
        let (text, needle) = (lhs.text(), rhs.text());
        if text.is_empty() || needle.is_empty() {
            return reparse(text.to_owned());
        }
        return reparse(text.replace(needle, ""));
    }
    if rhs.is_string() {
        return lhs;
    }
    numeric(&lhs, &rhs, i32::wrapping_sub, |a, b| a - b)
}

/// `*`: a `%.Ns` format keeps the right side's first N bytes, other text the
/// whole right side; else numeric multiplication.
fn times(lhs: Operand, rhs: Operand) -> Operand {
    if lhs.is_string() {
        let right = rhs.text();
        let keep = truncation(lhs.text()).map_or(right.len(), |count| count.min(right.len()));
        return reparse(String::from_utf8_lossy(&right.as_bytes()[..keep]).into_owned());
    }
    if rhs.is_string() {
        return lhs;
    }
    numeric(&lhs, &rhs, i32::wrapping_mul, |a, b| a * b)
}

/// `N` of a `%.Ns` format, parsed as `strtoull` does; a negative or
/// unparseable count keeps the whole text.
fn truncation(format: &str) -> Option<usize> {
    let middle = format.strip_prefix("%.")?.strip_suffix('s')?;
    let digits = middle.trim_start();
    if digits.starts_with('-') {
        return None;
    }
    let digits = digits.strip_prefix('+').unwrap_or(digits);
    let end = digits.bytes().take_while(u8::is_ascii_digit).count();
    if end == 0 {
        return None;
    }
    digits[..end]
        .parse::<u64>()
        .ok()
        .map(|count| count.min(usize::MAX as u64) as usize)
}

/// `/`: text counts non-overlapping matches (1 for no needle); a zero divisor
/// keeps the left side; else numeric division, integer when neither is float.
fn divide(lhs: Operand, rhs: Operand) -> Operand {
    if lhs.is_string() {
        let needle = rhs.text();
        if needle.is_empty() {
            return Operand::Int(1);
        }
        return Operand::Int(lhs.text().matches(needle).count() as i32);
    }
    if rhs.is_string() || rhs.float() == 0.0 {
        return lhs;
    }
    numeric(&lhs, &rhs, i32::wrapping_div, |a, b| a / b)
}
