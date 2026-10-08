//! Expression source to tokens, split and typed as the vanilla client does.

use serde_json::Value;

use super::{MAX_BYTES, MAX_NESTING, MAX_TOKENS};

/// Operator codes, as the vanilla client numbers them.
pub(super) const AND: u8 = 1;
pub(super) const OR: u8 = 2;
pub(super) const GREATER: u8 = 3;
pub(super) const LESS: u8 = 4;
pub(super) const EQUAL: u8 = 5;
pub(super) const PLUS: u8 = 6;
pub(super) const MINUS: u8 = 7;
pub(super) const TIMES: u8 = 8;
pub(super) const DIVIDE: u8 = 9;
pub(super) const NOT: u8 = 10;

/// One parsed token; a parenthesised group nests its own tokens.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Token {
    Value(Operand),
    /// A `#name` read from the property bag at evaluation.
    Property(String),
    /// A `$name` read from the variable scope at evaluation.
    Variable(String),
    Operator(u8),
    Group(Vec<Token>),
}

/// A typed value as an `ExprToken` carries it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Operand {
    Bool(bool),
    Int(i32),
    Float(f32),
    Str(String),
    /// Null, array or object JSON.
    Other(Value),
    /// A string starting with `#`: a property token, looked up only as a
    /// final result and otherwise an untyped string.
    PropertyText(String),
    /// A string result that parsed as an operator: untyped null.
    OperatorText(u8),
}

impl Operand {
    /// Vanilla JSON truthiness: nonzero, nonempty, or true.
    pub(super) fn truthy(&self) -> bool {
        match self {
            Operand::Bool(value) => *value,
            Operand::Int(value) => *value != 0,
            Operand::Float(value) => *value != 0.0,
            Operand::Str(text) | Operand::PropertyText(text) => !text.is_empty(),
            Operand::Other(Value::Array(items)) => !items.is_empty(),
            Operand::Other(Value::Object(map)) => !map.is_empty(),
            Operand::Other(_) | Operand::OperatorText(_) => false,
        }
    }

    /// Vanilla JSON int read: strings and compound values read 0.
    pub(super) fn int(&self) -> i32 {
        match self {
            Operand::Bool(value) => i32::from(*value),
            Operand::Int(value) => *value,
            Operand::Float(value) => *value as i32,
            _ => 0,
        }
    }

    /// Vanilla JSON float read: strings and compound values read 0.
    pub(super) fn float(&self) -> f32 {
        match self {
            Operand::Bool(value) => f32::from(u8::from(*value)),
            Operand::Int(value) => *value as f32,
            Operand::Float(value) => *value,
            _ => 0.0,
        }
    }

    /// The text a string operator reads: a bool's word, a string, else empty.
    pub(super) fn text(&self) -> &str {
        match self {
            Operand::Bool(true) => "true",
            Operand::Bool(false) => "false",
            Operand::Str(text) | Operand::PropertyText(text) => text,
            _ => "",
        }
    }

    pub(super) fn is_string(&self) -> bool {
        matches!(self, Operand::Str(_))
    }

    pub(super) fn is_float(&self) -> bool {
        matches!(self, Operand::Float(_))
    }

    pub(super) fn is_bool(&self) -> bool {
        matches!(self, Operand::Bool(_))
    }

    /// The token a JSON value becomes.
    pub(crate) fn from_json(value: &Value) -> Self {
        match value {
            Value::Bool(flag) => Operand::Bool(*flag),
            Value::Number(number) => match number.as_i64() {
                Some(int) => Operand::Int(int as i32),
                None => match number.as_u64() {
                    Some(uint) => Operand::Int(uint as i32),
                    None => Operand::Float(number.as_f64().unwrap_or(0.0) as f32),
                },
            },
            Value::String(text) => Operand::literal_text(text.clone()),
            other => Operand::Other(other.clone()),
        }
    }

    /// A stored string: `#`-prefixed text becomes a property token.
    pub(crate) fn literal_text(text: String) -> Self {
        if text.starts_with('#') {
            Operand::PropertyText(text)
        } else {
            Operand::Str(text)
        }
    }
}

/// Characters that end a bare token (`0x7000af1100000000` in the client).
fn delimiter(byte: u8) -> bool {
    matches!(
        byte,
        b' ' | b'$' | b'(' | b')' | b'*' | b'+' | b'-' | b'/' | b'<' | b'=' | b'>'
    )
}

fn operator_byte(byte: u8) -> bool {
    matches!(byte, b'*' | b'+' | b'-' | b'/' | b'<' | b'=' | b'>')
}

/// A surplus closing token discards adjacent bare text after a completed group.
/// Unclosed groups close at the end; a leading close stays undecidable.
pub(super) fn tokenize(source: &str) -> Option<Vec<Token>> {
    if source.len() > MAX_BYTES {
        return None;
    }
    let bytes = source.as_bytes();
    let mut groups: Vec<Vec<Token>> = vec![Vec::new()];
    let mut count = 0usize;
    let mut i = 0;
    while i < bytes.len() {
        let byte = bytes[i];
        if byte != b' ' {
            count += 1;
            if count > MAX_TOKENS {
                return None;
            }
        }
        match byte {
            b' ' => {
                i += 1;
                continue;
            }
            b'(' => {
                if groups.len() > MAX_NESTING {
                    return None;
                }
                groups.push(Vec::new());
                i += 1;
                continue;
            }
            b')' => {
                if groups.len() < 2 {
                    let completed_group = matches!(groups[0].last(), Some(Token::Group(_)));
                    if completed_group {
                        i += 1;
                        while i < bytes.len() && !delimiter(bytes[i]) {
                            i += 1;
                        }
                        continue;
                    }
                    return None;
                }
                let group = groups.pop()?;
                groups.last_mut()?.push(Token::Group(group));
                i += 1;
                continue;
            }
            _ => {}
        }
        let start = i;
        if operator_byte(byte) {
            i += 1;
        } else {
            if byte == b'\'' || byte == b'"' {
                i += 1;
                while i < bytes.len() && bytes[i] != byte {
                    i += 1;
                }
                i = (i + 1).min(bytes.len());
            } else {
                i += 1;
            }
            while i < bytes.len() && !delimiter(bytes[i]) {
                i += 1;
            }
        }
        let word = &source[start..i];
        let token = match word.strip_prefix('$') {
            Some(name) => Token::Variable(name.to_owned()),
            None => parse_word(word),
        };
        groups.last_mut()?.push(token);
    }
    while groups.len() > 1 {
        let group = groups.pop()?;
        groups.last_mut()?.push(Token::Group(group));
    }
    groups.pop()
}

/// Token typing order: keyword, quoted string, property, int, float,
/// bool word, operator, else bare string.
pub(super) fn parse_word(word: &str) -> Token {
    match word {
        "or" => return Token::Operator(OR),
        "and" => return Token::Operator(AND),
        "not" => return Token::Operator(NOT),
        _ => {}
    }
    let bytes = word.as_bytes();
    if let Some(&first) = bytes.first() {
        if (first == b'\'' || first == b'"') && bytes.len() > 1 && bytes[bytes.len() - 1] == first {
            return Token::Value(Operand::Str(word[1..word.len() - 1].to_owned()));
        }
        if first == b'#' {
            return Token::Property(word.to_owned());
        }
    }
    if let Some(int) = leading_int(word) {
        return Token::Value(Operand::Int(int));
    }
    if let Some(float) = leading_float(word) {
        return Token::Value(Operand::Float(float));
    }
    if let Some(flag) = word_bool(word) {
        return Token::Value(Operand::Bool(flag));
    }
    match bytes.first() {
        Some(b'*') => Token::Operator(TIMES),
        Some(b'+') => Token::Operator(PLUS),
        Some(b'-') => Token::Operator(MINUS),
        Some(b'/') => Token::Operator(DIVIDE),
        Some(b'<') => Token::Operator(LESS),
        Some(b'=') => Token::Operator(EQUAL),
        Some(b'>') => Token::Operator(GREATER),
        _ => Token::Value(Operand::Str(word.to_owned())),
    }
}

/// `strtol(…, 10)` accepting any parsed prefix, failing on overflow.
fn leading_int(word: &str) -> Option<i32> {
    let trimmed = word.trim_start();
    let bytes = trimmed.as_bytes();
    let mut end = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let digits = end;
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
    }
    if end == digits {
        return None;
    }
    trimmed[..end].parse::<i32>().ok()
}

/// `strtof` accepting any parsed prefix, failing on overflow.
fn leading_float(word: &str) -> Option<f32> {
    let trimmed = word.trim_start();
    let bytes = trimmed.as_bytes();
    let mut end = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let mut mantissa = false;
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
        mantissa = true;
    }
    if end < bytes.len() && bytes[end] == b'.' {
        end += 1;
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
            mantissa = true;
        }
    }
    if !mantissa {
        let lower = trimmed.to_ascii_lowercase();
        let unsigned = lower.trim_start_matches(['+', '-']);
        let negative = lower.starts_with('-');
        let special = if unsigned.starts_with("inf") {
            f32::INFINITY
        } else if unsigned.starts_with("nan") {
            f32::NAN
        } else {
            return None;
        };
        return Some(if negative { -special } else { special });
    }
    if end < bytes.len() && matches!(bytes[end], b'e' | b'E') {
        let mut exponent = end + 1;
        if exponent < bytes.len() && matches!(bytes[exponent], b'+' | b'-') {
            exponent += 1;
        }
        let digits = exponent;
        while exponent < bytes.len() && bytes[exponent].is_ascii_digit() {
            exponent += 1;
        }
        if exponent > digits {
            end = exponent;
        }
    }
    let value = trimmed[..end].parse::<f32>().ok()?;
    value.is_finite().then_some(value)
}

/// Vanilla bool words, case-insensitive: `true`/`false`, `yes`/`no`, `1`/`0`.
fn word_bool(word: &str) -> Option<bool> {
    match word.to_ascii_lowercase().as_str() {
        "true" | "yes" | "1" => Some(true),
        "false" | "no" | "0" => Some(false),
        _ => None,
    }
}

/// An operator's text result, reparsed as a
/// full token, empty text staying an empty string.
pub(super) fn reparse(text: String) -> Operand {
    if text.is_empty() {
        return Operand::Str(text);
    }
    match parse_word(&text) {
        Token::Value(operand) => operand,
        Token::Property(name) => Operand::PropertyText(name),
        Token::Operator(op) => Operand::OperatorText(op),
        Token::Variable(_) | Token::Group(_) => Operand::Str(text),
    }
}
