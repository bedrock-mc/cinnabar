//! Reader for `ui/*.json`, matching the vanilla client's comment-tolerant jsoncpp
//! reader: a leading UTF-8 BOM, `//` and `/* */` comments as token separators,
//! leading-zero numbers and raw control characters in strings are accepted, text
//! after the root value is ignored, and trailing commas and bare keys are errors.

use serde_json::{Map, Number, Value};

/// Deepest object/array nesting read; a deeper server document is rejected.
const MAX_DEPTH: usize = 1000;

#[derive(Debug, thiserror::Error)]
#[error("json parse error at line {line} column {column}: {message}")]
pub struct ParseError {
    message: &'static str,
    line: usize,
    column: usize,
    partial: Value,
}

/// Parse one document, ignoring anything after its root value.
pub fn parse(source: &str) -> Result<Value, ParseError> {
    let text = source.strip_prefix('\u{feff}').unwrap_or(source);
    let mut reader = Reader {
        bytes: text.as_bytes(),
        pos: 0,
        depth: 0,
    };
    reader.value()
}

/// Keeps the output constructed before an error without accepting invalid syntax.
pub(crate) fn parse_partial(source: &str) -> (Value, Option<ParseError>) {
    match parse(source) {
        Ok(value) => (value, None),
        Err(mut error) => (error.partial.take(), Some(error)),
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
    depth: usize,
}

impl Reader<'_> {
    fn error(&self, message: &'static str) -> ParseError {
        let before = &self.bytes[..self.pos.min(self.bytes.len())];
        let line = before.iter().filter(|&&byte| byte == b'\n').count() + 1;
        let column = before
            .iter()
            .rev()
            .take_while(|&&byte| byte != b'\n')
            .count()
            + 1;
        ParseError {
            message,
            line,
            column,
            partial: Value::Null,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn skip_spaces(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.pos += 1;
        }
    }

    /// Whitespace and comments before the next token.
    fn skip_insignificant(&mut self) -> Result<(), ParseError> {
        loop {
            self.skip_spaces();
            if self.peek() != Some(b'/') {
                return Ok(());
            }
            match self.bytes.get(self.pos + 1) {
                Some(b'/') => {
                    while self.peek().is_some_and(|byte| byte != b'\n') {
                        self.pos += 1;
                    }
                }
                Some(b'*') => {
                    self.pos += 2;
                    loop {
                        match self.peek() {
                            None => return Err(self.error("unterminated comment")),
                            Some(b'*') if self.bytes.get(self.pos + 1) == Some(&b'/') => {
                                self.pos += 2;
                                break;
                            }
                            Some(_) => self.pos += 1,
                        }
                    }
                }
                _ => return Err(self.error("unexpected `/`")),
            }
        }
    }

    fn value(&mut self) -> Result<Value, ParseError> {
        self.skip_insignificant()?;
        match self.peek() {
            Some(b'{') => self.nested(Self::object),
            Some(b'[') => self.nested(Self::array),
            Some(b'"') => self.string().map(Value::String),
            Some(b'0'..=b'9' | b'-') => Ok(self.number()),
            Some(b't') => self.literal("true", Value::Bool(true)),
            Some(b'f') => self.literal("false", Value::Bool(false)),
            Some(b'n') => self.literal("null", Value::Null),
            _ => Err(self.error("value, object or array expected")),
        }
    }

    fn nested(
        &mut self,
        read: fn(&mut Self) -> Result<Value, ParseError>,
    ) -> Result<Value, ParseError> {
        if self.depth >= MAX_DEPTH {
            return Err(self.error("nesting too deep"));
        }
        self.depth += 1;
        let value = read(self);
        self.depth -= 1;
        value
    }

    fn literal(&mut self, word: &str, value: Value) -> Result<Value, ParseError> {
        if self.bytes[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            Ok(value)
        } else {
            Err(self.error("invalid literal"))
        }
    }

    fn object(&mut self) -> Result<Value, ParseError> {
        self.pos += 1;
        let mut map = Map::new();
        match self.object_members(&mut map) {
            Ok(()) => Ok(Value::Object(map)),
            Err(mut error) => {
                error.partial = Value::Object(map);
                Err(error)
            }
        }
    }

    fn object_members(&mut self, map: &mut Map<String, Value>) -> Result<(), ParseError> {
        // jsoncpp accepts `}` where a member name is due only while the last name is empty.
        let mut name = String::new();
        loop {
            self.skip_insignificant()?;
            match self.peek() {
                Some(b'}') if name.is_empty() => {
                    self.pos += 1;
                    return Ok(());
                }
                Some(b'"') => name = self.string()?,
                _ => return Err(self.error("missing `}` or object member name")),
            }
            self.skip_insignificant()?;
            if self.peek() != Some(b':') {
                return Err(self.error("missing `:` after object member name"));
            }
            self.pos += 1;
            let value = match self.value() {
                Ok(value) => value,
                Err(mut error) => {
                    map.insert(name.clone(), error.partial.take());
                    return Err(error);
                }
            };
            map.insert(name.clone(), value);
            self.skip_insignificant()?;
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(());
                }
                _ => return Err(self.error("missing `,` or `}` in object")),
            }
        }
    }

    fn array(&mut self) -> Result<Value, ParseError> {
        self.pos += 1;
        let mut items = Vec::new();
        match self.array_items(&mut items) {
            Ok(()) => Ok(Value::Array(items)),
            Err(mut error) => {
                error.partial = Value::Array(items);
                Err(error)
            }
        }
    }

    fn array_items(&mut self, items: &mut Vec<Value>) -> Result<(), ParseError> {
        self.skip_insignificant()?;
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(());
        }
        loop {
            match self.value() {
                Ok(value) => items.push(value),
                Err(mut error) => {
                    items.push(error.partial.take());
                    return Err(error);
                }
            }
            self.skip_insignificant()?;
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(());
                }
                _ => return Err(self.error("missing `,` or `]` in array")),
            }
        }
    }

    /// Characters jsoncpp's number token spans, decoded as an integer when it
    /// has no fraction/exponent, else as the longest leading float.
    fn number(&mut self) -> Value {
        let start = self.pos;
        while matches!(
            self.peek(),
            Some(b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-')
        ) {
            self.pos += 1;
        }
        let token = std::str::from_utf8(&self.bytes[start..self.pos]).unwrap_or("");
        let integral = !token[1.min(token.len())..].contains(['.', 'e', 'E', '+', '-']);
        if integral {
            if let Ok(number) = token.parse::<i64>() {
                return Value::Number(number.into());
            }
            if let Ok(number) = token.parse::<u64>() {
                return Value::Number(number.into());
            }
        }
        let value = (1..=token.len())
            .rev()
            .find_map(|end| token[..end].parse::<f64>().ok())
            .unwrap_or(0.0);
        Number::from_f64(value).map_or(Value::Null, Value::Number)
    }

    fn string(&mut self) -> Result<String, ParseError> {
        self.pos += 1;
        let mut out = Vec::new();
        loop {
            let Some(byte) = self.peek() else {
                return Err(self.error("unterminated string"));
            };
            self.pos += 1;
            match byte {
                b'"' => break,
                b'\\' => {
                    let Some(escape) = self.peek() else {
                        return Err(self.error("unterminated string"));
                    };
                    self.pos += 1;
                    match escape {
                        b'"' | b'\\' | b'/' => out.push(escape),
                        b'b' => out.push(0x08),
                        b'f' => out.push(0x0c),
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'u' => {
                            let ch = self.unicode_escape()?;
                            let mut buffer = [0; 4];
                            out.extend_from_slice(ch.encode_utf8(&mut buffer).as_bytes());
                        }
                        _ => return Err(self.error("bad escape sequence in string")),
                    }
                }
                other => out.push(other),
            }
        }
        Ok(String::from_utf8(out)
            .unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned()))
    }

    fn hex4(&mut self) -> Result<u32, ParseError> {
        let digits = self
            .bytes
            .get(self.pos..self.pos + 4)
            .and_then(|digits| std::str::from_utf8(digits).ok())
            .and_then(|digits| u32::from_str_radix(digits, 16).ok())
            .ok_or_else(|| self.error("bad unicode escape sequence"))?;
        self.pos += 4;
        Ok(digits)
    }

    fn unicode_escape(&mut self) -> Result<char, ParseError> {
        let high = self.hex4()?;
        let code = if (0xd800..0xdc00).contains(&high) {
            if !self.bytes[self.pos..].starts_with(b"\\u") {
                return Err(self.error("missing low surrogate"));
            }
            self.pos += 2;
            let low = self.hex4()?;
            0x10000 + ((high & 0x3ff) << 10) + (low & 0x3ff)
        } else {
            high
        };
        char::from_u32(code).ok_or_else(|| self.error("bad unicode escape sequence"))
    }
}

#[cfg(test)]
mod tests {
    use super::parse;
    use serde_json::json;

    #[test]
    fn line_comments_and_banner_chars_are_ignored() {
        let value =
            parse("/****\n+* banner *\n****/\n{\n  // a field\n  \"a\": 1 // trailing note\n}")
                .unwrap();
        assert_eq!(value["a"], 1);
    }

    #[test]
    fn slashes_and_commas_inside_strings_survive() {
        let value = parse("{ \"expr\": \"(not (#a = '') or b),\" }").unwrap();
        assert_eq!(value["expr"], "(not (#a = '') or b),");
    }

    // jsoncpp's object and array readers reject a trailing comma.
    #[test]
    fn trailing_commas_are_syntax_errors() {
        assert!(parse(r#"{"namespace":"a","c":{"type":"panel",}}"#).is_err());
        assert!(parse(r#"{"namespace":"a","c":{"controls":[{"x":{}},]}}"#).is_err());
    }

    // A comment separates tokens rather than joining their fragments.
    #[test]
    fn comments_do_not_join_tokens() {
        assert!(parse(r#"{"flag":tr/* comment */ue}"#).is_err());
        assert!(parse("{\"n\":1/* c */2}").is_err());
        assert_eq!(parse("{\"n\":/* c */2}").unwrap(), json!({"n": 2}));
    }

    // Leading zeros, raw newlines in strings, and text after the root are tolerated.
    #[test]
    fn native_reader_tolerances_are_kept() {
        assert_eq!(parse(r#"{"n":01}"#).unwrap(), json!({"n": 1}));
        assert_eq!(
            parse("{\"text\":\"a\nb\"}").unwrap(),
            json!({"text": "a\nb"})
        );
        assert_eq!(
            parse(r#"{"namespace":"a","c":{}} {}"#).unwrap(),
            json!({"namespace": "a", "c": {}})
        );
    }

    #[test]
    fn a_leading_byte_order_mark_is_stripped() {
        let text = "\u{feff}{\"namespace\":\"a\",\"c\":{\"type\":\"panel\"}}";
        assert_eq!(parse(text).unwrap()["c"]["type"], "panel");
    }

    #[test]
    fn comments_before_an_empty_array_are_token_separators() {
        assert_eq!(parse("[ /* comment */ ]").unwrap(), json!([]));
        assert_eq!(parse("[ // comment\n ]").unwrap(), json!([]));
    }

    #[test]
    fn numbers_escapes_and_nesting_decode() {
        let value = parse(r#"{"i":-3,"f":1.5e1,"s":"é\n","a":[true,false,null]}"#).unwrap();
        assert_eq!(
            value,
            json!({"i": -3, "f": 15.0, "s": "é\n", "a": [true, false, null]})
        );
        let deep = format!("{}{}", "[".repeat(5000), "]".repeat(5000));
        assert!(parse(&deep).is_err());
    }
}
