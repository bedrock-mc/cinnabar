//! Position-keeping reader for the tolerant JSON the `ui/*.json` files use
//! (comments and trailing commas), so definitions and properties map back to
//! byte ranges. Values are validated by the engine's own parser; this one only
//! locates keys.

/// A parsed value with its byte range in the source.
#[derive(Debug)]
pub struct Spanned {
    pub start: usize,
    pub end: usize,
    pub node: Node,
}

#[derive(Debug)]
pub enum Node {
    Object(Vec<Member>),
    Array(Vec<Spanned>),
    String(String),
    Other,
}

/// One object entry: the key text and range, plus its value.
#[derive(Debug)]
pub struct Member {
    pub key: String,
    pub key_start: usize,
    pub key_end: usize,
    pub value: Spanned,
}

impl Spanned {
    pub fn members(&self) -> &[Member] {
        match &self.node {
            Node::Object(members) => members,
            _ => &[],
        }
    }

    pub fn items(&self) -> &[Spanned] {
        match &self.node {
            Node::Array(items) => items,
            _ => &[],
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match &self.node {
            Node::String(text) => Some(text),
            _ => None,
        }
    }

    pub fn get(&self, key: &str) -> Option<&Spanned> {
        self.members()
            .iter()
            .find(|member| member.key == key)
            .map(|member| &member.value)
    }
}

/// A syntax error at a byte offset.
#[derive(Debug, Clone, PartialEq)]
pub struct SyntaxError {
    pub offset: usize,
    pub message: String,
}

/// Parse `source`; the first syntax error stops the parse.
pub fn parse(source: &str) -> Result<Spanned, SyntaxError> {
    let mut parser = Parser {
        bytes: source.as_bytes(),
        source,
        pos: 0,
        depth: 0,
    };
    parser.skip_trivia()?;
    let value = parser.value()?;
    parser.skip_trivia()?;
    if parser.pos < parser.bytes.len() {
        return Err(parser.error("unexpected text after the document"));
    }
    Ok(value)
}

/// Zero-based line and UTF-16 column of `offset`, as editors count them.
pub fn line_col(source: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(source.len());
    let before = &source[..floor_boundary(source, offset)];
    let line = before.matches('\n').count();
    let line_start = before.rfind('\n').map_or(0, |index| index + 1);
    let column = before[line_start..].encode_utf16().count();
    (line, column)
}

fn floor_boundary(source: &str, mut offset: usize) -> usize {
    while !source.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

const MAX_DEPTH: usize = 512;

struct Parser<'a> {
    bytes: &'a [u8],
    source: &'a str,
    pos: usize,
    depth: usize,
}

impl Parser<'_> {
    fn error(&self, message: &str) -> SyntaxError {
        SyntaxError {
            offset: self.pos,
            message: message.to_owned(),
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn skip_trivia(&mut self) -> Result<(), SyntaxError> {
        loop {
            match (self.peek(), self.bytes.get(self.pos + 1)) {
                (Some(b' ' | b'\t' | b'\n' | b'\r'), _) => self.pos += 1,
                (Some(b'/'), Some(b'/')) => {
                    while self.peek().is_some_and(|byte| byte != b'\n') {
                        self.pos += 1;
                    }
                }
                (Some(b'/'), Some(b'*')) => {
                    let start = self.pos;
                    self.pos += 2;
                    loop {
                        match (self.peek(), self.bytes.get(self.pos + 1)) {
                            (Some(b'*'), Some(b'/')) => {
                                self.pos += 2;
                                break;
                            }
                            (Some(_), _) => self.pos += 1,
                            (None, _) => {
                                self.pos = start;
                                return Err(self.error("unterminated block comment"));
                            }
                        }
                    }
                }
                // Header banners carry stray non-ASCII bytes outside strings.
                (Some(byte), _) if byte >= 0x80 => self.pos += 1,
                _ => return Ok(()),
            }
        }
    }

    fn value(&mut self) -> Result<Spanned, SyntaxError> {
        let start = self.pos;
        let node = match self.peek() {
            Some(b'{') => self.object()?,
            Some(b'[') => self.array()?,
            Some(b'"') => Node::String(self.string()?),
            Some(b'-' | b'0'..=b'9') => {
                self.number()?;
                Node::Other
            }
            Some(b't') => self.literal("true")?,
            Some(b'f') => self.literal("false")?,
            Some(b'n') => self.literal("null")?,
            Some(_) => return Err(self.error("expected a value")),
            None => return Err(self.error("unexpected end of document")),
        };
        Ok(Spanned {
            start,
            end: self.pos,
            node,
        })
    }

    fn enter(&mut self) -> Result<(), SyntaxError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(self.error("nesting too deep"));
        }
        Ok(())
    }

    fn object(&mut self) -> Result<Node, SyntaxError> {
        self.enter()?;
        self.pos += 1;
        let mut members = Vec::new();
        loop {
            self.skip_trivia()?;
            match self.peek() {
                Some(b'}') => {
                    self.pos += 1;
                    break;
                }
                Some(b'"') => {}
                _ => return Err(self.error("expected a quoted key or `}`")),
            }
            let key_start = self.pos;
            let key = self.string()?;
            let key_end = self.pos;
            self.skip_trivia()?;
            if self.peek() != Some(b':') {
                return Err(self.error("expected `:` after the key"));
            }
            self.pos += 1;
            self.skip_trivia()?;
            let value = self.value()?;
            members.push(Member {
                key,
                key_start,
                key_end,
                value,
            });
            self.skip_trivia()?;
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {}
                _ => return Err(self.error("expected `,` or `}`")),
            }
        }
        self.depth -= 1;
        Ok(Node::Object(members))
    }

    fn array(&mut self) -> Result<Node, SyntaxError> {
        self.enter()?;
        self.pos += 1;
        let mut items = Vec::new();
        loop {
            self.skip_trivia()?;
            if self.peek() == Some(b']') {
                self.pos += 1;
                break;
            }
            items.push(self.value()?);
            self.skip_trivia()?;
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {}
                _ => return Err(self.error("expected `,` or `]`")),
            }
        }
        self.depth -= 1;
        Ok(Node::Array(items))
    }

    fn string(&mut self) -> Result<String, SyntaxError> {
        let start = self.pos;
        self.pos += 1;
        let mut text = String::new();
        let mut run = self.pos;
        loop {
            match self.peek() {
                Some(b'"') => {
                    text.push_str(&self.source[run..self.pos]);
                    self.pos += 1;
                    return Ok(text);
                }
                Some(b'\\') => {
                    text.push_str(&self.source[run..self.pos]);
                    self.pos += 1;
                    let escaped = match self.peek() {
                        Some(b'n') => '\n',
                        Some(b't') => '\t',
                        Some(b'r') => '\r',
                        Some(b'b') => '\u{8}',
                        Some(b'f') => '\u{c}',
                        Some(b'u') => self.unicode_escape()?,
                        Some(byte @ (b'"' | b'\\' | b'/')) => char::from(byte),
                        _ => return Err(self.error("invalid escape")),
                    };
                    text.push(escaped);
                    self.pos += 1;
                    run = self.pos;
                }
                Some(b'\n') | None => {
                    self.pos = start;
                    return Err(self.error("unterminated string"));
                }
                Some(_) => self.pos += 1,
            }
        }
    }

    /// Reads `uXXXX` with `pos` on the `u`, leaving `pos` on the last hex digit.
    fn unicode_escape(&mut self) -> Result<char, SyntaxError> {
        let hex = self
            .source
            .get(self.pos + 1..self.pos + 5)
            .and_then(|digits| u32::from_str_radix(digits, 16).ok())
            .ok_or_else(|| self.error("invalid \\u escape"))?;
        self.pos += 4;
        let scalar = if (0xd800..=0xdbff).contains(&hex) {
            if self.source.get(self.pos + 1..self.pos + 3) != Some("\\u") {
                return Err(self.error("missing low surrogate"));
            }
            let low = self
                .source
                .get(self.pos + 3..self.pos + 7)
                .and_then(|digits| u32::from_str_radix(digits, 16).ok())
                .filter(|low| (0xdc00..=0xdfff).contains(low))
                .ok_or_else(|| self.error("invalid low surrogate"))?;
            self.pos += 6;
            0x10000 + ((hex - 0xd800) << 10) + low - 0xdc00
        } else {
            hex
        };
        char::from_u32(scalar).ok_or_else(|| self.error("unpaired surrogate"))
    }

    fn number(&mut self) -> Result<(), SyntaxError> {
        let start = self.pos;
        while self
            .peek()
            .is_some_and(|byte| matches!(byte, b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9'))
        {
            self.pos += 1;
        }
        if self.source[start..self.pos].parse::<f64>().is_err() {
            self.pos = start;
            return Err(self.error("invalid number"));
        }
        Ok(())
    }

    fn literal(&mut self, word: &str) -> Result<Node, SyntaxError> {
        if self.source[self.pos..].starts_with(word) {
            self.pos += word.len();
            Ok(Node::Other)
        } else {
            Err(self.error("expected a value"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Keys keep their byte ranges through comments and trailing commas.
    #[test]
    fn keys_keep_ranges_through_comments_and_trailing_commas() {
        let source = "/* banner */ { \"a\": [1, 2,], // note\n \"b@c.d\": { \"x\": \"y\", }, }";
        let root = parse(source).unwrap();
        let b = &root.members()[1];
        assert_eq!(&source[b.key_start..b.key_end], "\"b@c.d\"");
        assert_eq!(b.value.get("x").and_then(Spanned::as_str), Some("y"));
    }

    #[test]
    fn errors_report_line_and_column() {
        let source = "{\n  \"a\": 1\n  \"b\": 2\n}";
        let error = parse(source).unwrap_err();
        assert_eq!(line_col(source, error.offset), (2, 2));
    }
}

#[cfg(test)]
mod review_tests {
    #[test]
    fn review_supplementary_unicode_escapes_round_trip_and_unpaired_ones_fail() {
        assert_eq!(
            crate::export::parse(r#"{"text":"\uD83D\uDE00"}"#).unwrap()["text"],
            "😀"
        );
        for bad in [
            r#"{"text":"\uD83D"}"#,
            r#"{"text":"\uDE00"}"#,
            r#"{"text":"\uD83D\u0041"}"#,
        ] {
            assert!(crate::export::parse(bad).is_none());
        }
    }
}
