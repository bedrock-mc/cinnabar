//! Preprocessed GLSL tokens that remember their source line, and the emitter that turns them
//! back into text while recording which source line each output line came from.

/// Token payload. Literals keep their spelling so the emitter reproduces them exactly.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum TokKind {
    Ident(String),
    Int(String),
    Float(String),
    Punct(&'static str),
}

/// One token plus the 1-based line of the include-expanded source it came from; 0 marks
/// tokens the transformer synthesized.
#[derive(Clone, Debug)]
pub(crate) struct Tok {
    pub kind: TokKind,
    pub line: u32,
}

impl Tok {
    /// Whether this is the punctuator `p`.
    pub fn is(&self, p: &str) -> bool {
        matches!(&self.kind, TokKind::Punct(q) if *q == p)
    }

    /// The identifier text, if this is an identifier.
    pub fn ident(&self) -> Option<&str> {
        match &self.kind {
            TokKind::Ident(s) => Some(s),
            _ => None,
        }
    }

    /// The token's spelling.
    pub fn text(&self) -> &str {
        match &self.kind {
            TokKind::Ident(s) | TokKind::Int(s) | TokKind::Float(s) => s,
            TokKind::Punct(p) => p,
        }
    }

    /// The same kind of token at another source line.
    pub fn at(mut self, line: u32) -> Tok {
        self.line = line;
        self
    }
}

/// A synthesized identifier token.
pub(crate) fn id(s: &str) -> Tok {
    Tok {
        kind: TokKind::Ident(s.to_string()),
        line: 0,
    }
}

/// A synthesized punctuator token.
pub(crate) fn p(s: &'static str) -> Tok {
    Tok {
        kind: TokKind::Punct(s),
        line: 0,
    }
}

/// A synthesized integer literal token.
pub(crate) fn int(value: impl ToString) -> Tok {
    Tok {
        kind: TokKind::Int(value.to_string()),
        line: 0,
    }
}

const PUNCTS: &[&str] = &[
    "!=", "==", "<=", ">=", "&&", "||", "(", ")", "[", "]", "{", "}", ";", ",", "=", "*", ".", "<",
    ">", "+", "-", "/", "!", "?", ":",
];

/// Lexes transformer-generated GLSL snippets (never pack text). Tokens get source line 0.
pub(crate) fn lex(src: &str) -> Vec<Tok> {
    let mut out = vec![];
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_whitespace() {
            i += 1;
        } else if c.is_ascii_alphabetic() || c == b'_' {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            out.push(id(&src[start..i]));
        } else if c.is_ascii_digit() {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'.') {
                i += 1;
            }
            let s = src[start..i].to_string();
            let kind = if s.contains('.') {
                TokKind::Float(s)
            } else {
                TokKind::Int(s)
            };
            out.push(Tok { kind, line: 0 });
        } else {
            let punct = PUNCTS
                .iter()
                .find(|p| src[i..].starts_with(**p))
                .unwrap_or_else(|| panic!("generated GLSL uses an unknown punctuator in {src:?}"));
            out.push(p(punct));
            i += punct.len();
        }
    }
    out
}

/// Accumulates GLSL text and, for every output line, the include-expanded source line it
/// came from (0 when synthesized), so naga diagnostics can be mapped back to pack files.
#[derive(Default)]
pub(crate) struct Emitter {
    text: String,
    origins: Vec<u32>,
    line_origin: u32,
}

impl Emitter {
    /// Appends generated text; its lines have no source origin.
    pub fn raw(&mut self, s: &str) {
        for ch in s.chars() {
            self.push_char(ch);
        }
    }

    /// Appends a character and records the origin of each completed line.
    fn push_char(&mut self, ch: char) {
        self.text.push(ch);
        if ch == '\n' {
            self.origins.push(self.line_origin);
            self.line_origin = 0;
        }
    }

    /// Reports whether the next character starts a new output line.
    fn at_line_start(&self) -> bool {
        self.text.is_empty() || self.text.ends_with('\n')
    }

    /// Appends tokens, one statement or brace per line, indented by brace depth.
    pub fn tokens(&mut self, toks: &[Tok]) {
        let mut indent = 0usize;
        let mut paren = 0i32;
        if !self.at_line_start() {
            self.push_char('\n');
        }
        for (i, t) in toks.iter().enumerate() {
            if t.is("}") {
                indent = indent.saturating_sub(1);
                if !self.at_line_start() {
                    self.push_char('\n');
                }
            }
            if self.at_line_start() {
                for _ in 0..indent {
                    self.text.push_str("    ");
                }
            } else if needs_space(&toks[i - 1], t) {
                self.text.push(' ');
            }
            if self.line_origin == 0 {
                self.line_origin = t.line;
            }
            self.text.push_str(t.text());
            if t.is("(") {
                paren += 1;
            } else if t.is(")") {
                paren -= 1;
            }
            if t.is("{") {
                indent += 1;
                self.push_char('\n');
            } else if t.is("}") || (t.is(";") && paren == 0) {
                self.push_char('\n');
            }
        }
        if !self.at_line_start() {
            self.push_char('\n');
        }
    }

    /// The text and the per-line source origins.
    pub fn finish(mut self) -> (String, Vec<u32>) {
        if !self.at_line_start() {
            self.push_char('\n');
        }
        (self.text, self.origins)
    }
}

/// Reports whether adjacent emitted tokens need separating whitespace.
fn needs_space(prev: &Tok, t: &Tok) -> bool {
    let tight = t.is(";")
        || t.is(",")
        || t.is(")")
        || t.is("]")
        || t.is(".")
        || prev.is("(")
        || prev.is("[")
        || prev.is(".")
        || (t.is("(") && matches!(prev.kind, TokKind::Ident(_)))
        || (t.is("[") && !prev.is("="))
        || t.is("++")
        || t.is("--")
        || prev.is("!")
        || prev.is("~");
    !tight
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emitter_records_the_first_source_line_of_each_output_line() {
        let mut toks = lex("float a = 1.0; void f() { a = 2.0; }");
        for (i, t) in toks.iter_mut().enumerate() {
            t.line = if i < 5 { 7 } else { 9 };
        }
        let mut e = Emitter::default();
        e.raw("#version 450\n");
        e.tokens(&toks);
        let (text, origins) = e.finish();
        assert_eq!(text.lines().count(), origins.len());
        assert_eq!(origins[0], 0);
        assert_eq!(origins[1], 7);
        assert_eq!(text.lines().nth(1), Some("float a = 1.0;"));
        assert!(origins[2..].iter().all(|o| *o == 9));
    }
}
