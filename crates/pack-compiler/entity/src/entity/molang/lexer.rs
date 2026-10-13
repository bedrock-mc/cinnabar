use assets::AssetError;

use crate::entity::invalid;

const MAX_SOURCE_BYTES: usize = 32 * 1024;
const MAX_STRING_BYTES: usize = assets::MAX_MOLANG_STRING_BYTES;

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Token {
    Number(f32),
    String(Box<str>),
    Identifier(Box<str>),
    LeftParen,
    RightParen,
    LeftBrace,
    RightBrace,
    LeftBracket,
    RightBracket,
    Comma,
    Semicolon,
    Question,
    Colon,
    Arrow,
    Assign,
    Operator(&'static str),
    End,
}

/// Splits source text into tokens; identifiers are lowercased and short namespaces expanded,
/// string literals keep their case.
pub(super) fn tokenize(source: &str) -> Result<Vec<Token>, AssetError> {
    if source.trim().is_empty() || source.len() > MAX_SOURCE_BYTES {
        return Err(invalid("Molang source size exceeds bound"));
    }
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        let byte = bytes[cursor];
        if byte.is_ascii_whitespace() {
            cursor += 1;
            continue;
        }
        let (token, length) = match byte {
            b'(' => (Token::LeftParen, 1),
            b')' => (Token::RightParen, 1),
            b'{' => (Token::LeftBrace, 1),
            b'}' => (Token::RightBrace, 1),
            b'[' => (Token::LeftBracket, 1),
            b']' => (Token::RightBracket, 1),
            b',' => (Token::Comma, 1),
            b';' => (Token::Semicolon, 1),
            b':' => (Token::Colon, 1),
            b'\'' => {
                let end = bytes[cursor + 1..]
                    .iter()
                    .position(|byte| *byte == b'\'')
                    .ok_or_else(|| invalid("unterminated Molang string"))?;
                let text = &source[cursor + 1..cursor + 1 + end];
                if text.len() > MAX_STRING_BYTES || text.chars().any(char::is_control) {
                    return Err(invalid("Molang string literal exceeds its bound"));
                }
                (Token::String(text.into()), end + 2)
            }
            _ => operator(&bytes[cursor..])
                .map(Ok)
                .or_else(|| {
                    (byte.is_ascii_digit() || byte == b'.').then(|| number(&source[cursor..]))
                })
                .or_else(|| {
                    (byte.is_ascii_alphabetic() || byte == b'_')
                        .then(|| Ok(identifier(&source[cursor..])))
                })
                .ok_or_else(|| invalid("unsupported token in Molang expression"))??,
        };
        tokens.push(token);
        cursor += length;
    }
    tokens.push(Token::End);
    Ok(tokens)
}

fn operator(bytes: &[u8]) -> Option<(Token, usize)> {
    for (text, token) in [
        (b"->".as_slice(), Token::Arrow),
        (b"??", Token::Operator("??")),
        (b"&&", Token::Operator("&&")),
        (b"||", Token::Operator("||")),
        (b"<=", Token::Operator("<=")),
        (b">=", Token::Operator(">=")),
        (b"==", Token::Operator("==")),
        (b"!=", Token::Operator("!=")),
    ] {
        if bytes.starts_with(text) {
            return Some((token, text.len()));
        }
    }
    let token = match bytes[0] {
        b'?' => Token::Question,
        b'=' => Token::Assign,
        b'+' => Token::Operator("+"),
        b'-' => Token::Operator("-"),
        b'*' => Token::Operator("*"),
        b'/' => Token::Operator("/"),
        b'<' => Token::Operator("<"),
        b'>' => Token::Operator(">"),
        b'!' => Token::Operator("!"),
        _ => return None,
    };
    Some((token, 1))
}

fn number(text: &str) -> Result<(Token, usize), AssetError> {
    let bytes = text.as_bytes();
    let mut end = 1;
    while end < bytes.len()
        && (bytes[end].is_ascii_digit()
            || bytes[end] == b'.'
            || matches!(bytes[end], b'e' | b'E')
            || (matches!(bytes[end], b'+' | b'-') && matches!(bytes[end - 1], b'e' | b'E')))
    {
        end += 1;
    }
    let value = text[..end]
        .parse::<f32>()
        .map_err(|_| invalid("invalid Molang numeric literal"))?;
    if !value.is_finite() {
        return Err(invalid("non-finite Molang numeric literal"));
    }
    // Authored float suffixes such as `1.0f` carry no Molang meaning.
    let suffix = usize::from(
        bytes
            .get(end)
            .is_some_and(|byte| matches!(byte, b'f' | b'F')),
    );
    Ok((Token::Number(value), end + suffix))
}

fn identifier(text: &str) -> (Token, usize) {
    let length = text
        .bytes()
        .position(|byte| !(byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.')))
        .unwrap_or(text.len());
    let lowered = text[..length].to_ascii_lowercase();
    let expanded = [
        ("q.", "query."),
        ("v.", "variable."),
        ("t.", "temp."),
        ("c.", "context."),
    ]
    .into_iter()
    .find_map(|(short, long)| {
        lowered
            .strip_prefix(short)
            .map(|rest| format!("{long}{rest}"))
    })
    .unwrap_or(lowered);
    (Token::Identifier(expanded.into()), length)
}
