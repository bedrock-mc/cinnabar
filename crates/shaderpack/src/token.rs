//! Preprocessed GLSL tokens that remember their source line, and the emitter that turns them
//! back into text while recording which source line each output line came from.

use pp_rs::pp::Preprocessor;
use pp_rs::token::{Punct, TokenValue};

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

/// Lexes transformer-generated GLSL snippets with the same lexer as pack code.
/// Tokens retain numeric spelling and receive source line zero.
pub(crate) fn lex(src: &str) -> Vec<Tok> {
    Preprocessor::new(src)
        .map(|token| {
            let token = token.expect("generated GLSL must tokenize");
            let spelling = &src[token.location.start as usize..token.location.end as usize];
            let kind = match token.value {
                TokenValue::Ident(name) => TokKind::Ident(name),
                TokenValue::Integer(value) => {
                    TokKind::Int(if spelling.starts_with(|ch: char| ch.is_ascii_digit()) {
                        spelling.to_string()
                    } else if value.signed {
                        value.value.to_string()
                    } else {
                        format!("{}u", value.value)
                    })
                }
                TokenValue::Float(_) => TokKind::Float(spelling.to_string()),
                TokenValue::Punct(value) => TokKind::Punct(punct(value)),
                _ => unreachable!("generated snippets contain no directives"),
            };
            Tok { kind, line: 0 }
        })
        .collect()
}

/// Returns the GLSL spelling of a punctuation token.
pub(crate) fn punct(p: Punct) -> &'static str {
    use Punct::*;
    match p {
        AddAssign => "+=",
        SubAssign => "-=",
        MulAssign => "*=",
        DivAssign => "/=",
        ModAssign => "%=",
        LeftShiftAssign => "<<=",
        RightShiftAssign => ">>=",
        AndAssign => "&=",
        XorAssign => "^=",
        OrAssign => "|=",
        Increment => "++",
        Decrement => "--",
        LogicalAnd => "&&",
        LogicalOr => "||",
        LogicalXor => "^^",
        LessEqual => "<=",
        GreaterEqual => ">=",
        EqualEqual => "==",
        NotEqual => "!=",
        LeftShift => "<<",
        RightShift => ">>",
        LeftBrace => "{",
        RightBrace => "}",
        LeftParen => "(",
        RightParen => ")",
        LeftBracket => "[",
        RightBracket => "]",
        LeftAngle => "<",
        RightAngle => ">",
        Semicolon => ";",
        Comma => ",",
        Colon => ":",
        Dot => ".",
        Equal => "=",
        Bang => "!",
        Minus => "-",
        Tilde => "~",
        Plus => "+",
        Star => "*",
        Slash => "/",
        Percent => "%",
        Pipe => "|",
        Caret => "^",
        Ampersand => "&",
        Question => "?",
    }
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

    /// Appends tokens, one statement or brace per line.
    pub fn tokens(&mut self, toks: &[Tok]) {
        let mut paren = 0i32;
        if !self.at_line_start() {
            self.push_char('\n');
        }
        for (i, t) in toks.iter().enumerate() {
            if t.is("}") && !self.at_line_start() {
                self.push_char('\n');
            }
            if !self.at_line_start() && needs_space(&toks[i - 1], t) {
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
            if t.is("{") || t.is("}") || (t.is(";") && paren == 0) {
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
    if t.is(".") && matches!(prev.kind, TokKind::Int(_))
        || prev.is(".") && matches!(t.kind, TokKind::Int(_) | TokKind::Float(_))
    {
        return true;
    }
    let tight = t.is(";")
        || t.is(",")
        || t.is(")")
        || t.is("]")
        || t.is(".")
        || prev.is("(")
        || prev.is("[")
        || prev.is(".")
        || (t.is("(") && matches!(prev.kind, TokKind::Ident(_)))
        || (t.is("[") && !prev.is("="));
    !tight
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontend_emitter_size_is_linear_for_nested_blocks() {
        for depth in [64, 512] {
            let source = format!(
                "{}float value = 1.0;{}",
                "{".repeat(depth),
                "}".repeat(depth)
            );
            let tokens = lex(&source);
            let mut emitter = Emitter::default();
            emitter.tokens(&tokens);
            let (text, _) = emitter.finish();
            assert!(
                text.len() <= 4 * source.len(),
                "depth {depth}: {} bytes",
                text.len()
            );
            assert_eq!(
                lex(&text)
                    .into_iter()
                    .map(|token| token.kind)
                    .collect::<Vec<_>>(),
                tokens
                    .into_iter()
                    .map(|token| token.kind)
                    .collect::<Vec<_>>()
            );
        }
    }

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

#[cfg(test)]
mod boundary_tests {
    use super::*;

    #[test]
    fn frontend_numeric_member_boundaries_survive_relexing() {
        for source in ["1 .xxx", "0x1 .x", "1u .xx", "1.0 .xxx", ". 1"] {
            let expected: Vec<_> = Preprocessor::new(source)
                .map(|token| token.unwrap().value)
                .collect();
            let mut emitter = Emitter::default();
            emitter.tokens(&lex(source));
            let (text, _) = emitter.finish();
            let actual: Vec<_> = Preprocessor::new(&text)
                .map(|token| token.unwrap().value)
                .collect();
            assert_eq!(actual, expected, "{text}");
        }
    }

    #[test]
    fn frontend_operator_boundaries() {
        for (a, b) in [("+", "++"), ("-", "--"), ("!", "="), ("~", "~")] {
            let tokens = vec![id("a"), p(a), p(b), id("b")];
            let mut emitter = Emitter::default();
            emitter.tokens(&tokens);
            let (text, _) = emitter.finish();
            let parsed: Vec<_> = pp_rs::pp::Preprocessor::new(&text)
                .map(|token| token.unwrap().value)
                .collect();
            let expected: Vec<_> = tokens
                .iter()
                .flat_map(|token| pp_rs::pp::Preprocessor::new(token.text()))
                .map(|token| token.unwrap().value)
                .collect();
            assert_eq!(parsed, expected, "{text}");
        }
    }
}

#[cfg(test)]
mod generated_tests {
    use super::*;

    #[test]
    fn generated_lexer_preserves_glsl_token_values() {
        for source in [
            "float x = 1e-3; x <<= 2; ++x; --x;",
            "a /* hidden */ ++ + b;",
        ] {
            let expected: Vec<_> = pp_rs::pp::Preprocessor::new(source)
                .map(|token| token.unwrap().value)
                .collect();
            let mut emitter = Emitter::default();
            emitter.tokens(&lex(source));
            let (text, _) = emitter.finish();
            let actual: Vec<_> = pp_rs::pp::Preprocessor::new(&text)
                .map(|token| token.expect("valid generated tokens").value)
                .collect();
            assert_eq!(actual, expected);
        }
    }
}
