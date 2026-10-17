//! Iris-style `#include` expansion, option overrides, comment directives and full macro and
//! conditional expansion with pp-rs (the preprocessor naga's GLSL frontend uses).

use crate::diagnostics::{Diagnostic, Phase, SourceLocation};
use crate::options::{OptionValues, PackOption, rewrite_option_line};
use crate::source::PackSource;
use crate::token::{Tok, TokKind};
use pp_rs::pp::Preprocessor;
use pp_rs::token::{Punct, TokenValue};

/// Identifier prefix of the markers that stand in for comment and `const` directives while
/// the preprocessor decides which of them sit in active code. Stripped before transforming.
const MARKER: &str = "__shaderpack_directive_";

/// A pack directive found in source text (`/* RENDERTARGETS: … */`, `/* DRAWBUFFERS:… */`,
/// or a `const <type> <name> = <value>;` line, commented or not).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Directive {
    RenderTargets(Vec<u32>),
    DrawBuffers(Vec<u32>),
    Const(ConstDirective),
}

/// One `const` directive line, as written (OptiFine/Iris read these even inside comments).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConstDirective {
    pub ty: String,
    pub name: String,
    pub value: String,
}

/// Maps lines of the include-expanded text back to pack files.
#[derive(Clone, Debug, Default)]
pub(crate) struct LineMap {
    files: Vec<String>,
    /// Per expanded line (index = line - 1): (index into `files`, 1-based line).
    lines: Vec<(u32, u32)>,
}

impl LineMap {
    /// The pack location of a 1-based expanded line.
    pub fn locate(&self, line: u32) -> Option<SourceLocation> {
        let (file, l) = *self.lines.get(line.checked_sub(1)? as usize)?;
        Some(SourceLocation {
            file: self.files[file as usize].clone(),
            line: l,
        })
    }

    fn file_index(&mut self, path: &str) -> u32 {
        match self.files.iter().position(|f| f == path) {
            Some(i) => i as u32,
            None => {
                self.files.push(path.to_string());
                (self.files.len() - 1) as u32
            }
        }
    }
}

/// A stage file after `#include` expansion.
pub(crate) struct Expanded {
    pub text: String,
    pub map: LineMap,
    directives: Vec<Directive>,
}

/// A stage file after full preprocessing.
pub(crate) struct Preprocessed {
    pub version: u32,
    pub profile: Option<String>,
    pub extensions: Vec<String>,
    pub tokens: Vec<Tok>,
    pub map: LineMap,
    /// The draw-buffer list from the last active RENDERTARGETS or DRAWBUFFERS directive.
    pub render_targets: Option<Vec<u32>>,
    /// Active `const` directives in source order.
    pub consts: Vec<ConstDirective>,
}

struct Expander<'a> {
    source: &'a PackSource,
    options: &'a [PackOption],
    values: &'a OptionValues,
    out: String,
    map: LineMap,
    directives: Vec<Directive>,
    stack: Vec<String>,
    pending: Vec<usize>,
    in_block: bool,
}

/// Expands `#include`s of the `shaders/`-relative `path` textually, like Iris: absolute
/// includes start at `shaders/`, relative ones at the including file, and includes inside
/// inactive `#if` groups expand too. Option definitions are rewritten to `values`.
pub(crate) fn expand(
    source: &PackSource,
    path: &str,
    options: &[PackOption],
    values: &OptionValues,
) -> Result<Expanded, Diagnostic> {
    let mut x = Expander {
        source,
        options,
        values,
        out: String::new(),
        map: LineMap::default(),
        directives: Vec::new(),
        stack: Vec::new(),
        pending: Vec::new(),
        in_block: false,
    };
    x.file(path, None)?;
    Ok(Expanded {
        text: x.out,
        map: x.map,
        directives: x.directives,
    })
}

impl Expander<'_> {
    fn file(&mut self, path: &str, from: Option<SourceLocation>) -> Result<(), Diagnostic> {
        let Some(text) = self.source.text(path) else {
            return Err(Diagnostic::new(Phase::Include, format!("cannot find `{path}`")).at(from));
        };
        if self.stack.iter().any(|p| p == path) {
            return Err(
                Diagnostic::new(Phase::Include, format!("include cycle through `{path}`")).at(from),
            );
        }
        self.stack.push(path.to_string());
        let file = self.map.file_index(path);
        for (n, line) in text.lines().enumerate() {
            let here = SourceLocation {
                file: path.to_string(),
                line: n as u32 + 1,
            };
            let trimmed = line.trim_start();
            if let Some(rest) = trimmed.strip_prefix("#include") {
                let target = resolve_include(path, rest);
                self.file(&target, Some(here))?;
                continue;
            }
            let rewritten = if self.values.is_empty() {
                None
            } else {
                rewrite_option_line(line, self.options, self.values)
            };
            let line = rewritten.as_deref().unwrap_or(line);
            let line = if trimmed.starts_with("#error") {
                // pp-rs tokenizes lines inside skipped groups and rejects the `"` of `#error "…"`.
                line.replace('"', "")
            } else {
                line.to_string()
            };
            let out = self.directives_in(&line);
            self.out.push_str(&out);
            self.out.push('\n');
            self.map.lines.push((file, n as u32 + 1));
        }
        self.stack.pop();
        Ok(())
    }

    /// Records the directives on `line` and returns the line with marker identifiers placed
    /// in code right after the comment that held them, so only directives in active code
    /// survive preprocessing.
    fn directives_in(&mut self, line: &str) -> String {
        let started_in_block = self.in_block;
        let scan = scan_line(line, &mut self.in_block);
        for comment in &scan.block_comments {
            if let Some(d) = comment_directive(comment) {
                self.directives.push(d);
                self.pending.push(self.directives.len() - 1);
            }
        }
        let trimmed = line.trim_start();
        if let Some(c) = trimmed.strip_prefix("const ").and_then(parse_const) {
            self.directives.push(Directive::Const(c));
            self.pending.push(self.directives.len() - 1);
        }
        let preprocessor_line = !started_in_block && trimmed.starts_with('#');
        if self.pending.is_empty()
            || self.in_block
            || preprocessor_line
            || line.trim_end().ends_with('\\')
        {
            return line.to_string();
        }
        let mut out = line[..scan.code_end].to_string();
        for i in self.pending.drain(..) {
            out.push_str(&format!(" {MARKER}{i} "));
        }
        out.push_str(&line[scan.code_end..]);
        out
    }
}

/// `shaders/`-relative target of an `#include` argument written in `from`.
fn resolve_include(from: &str, arg: &str) -> String {
    let name = arg
        .trim()
        .trim_matches(|c| c == '"' || c == '<' || c == '>');
    let joined = match name.strip_prefix('/') {
        Some(abs) => abs.to_string(),
        None => match from.rsplit_once('/') {
            Some((dir, _)) => format!("{dir}/{name}"),
            None => name.to_string(),
        },
    };
    let mut parts: Vec<&str> = Vec::new();
    for seg in joined.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

struct LineScan {
    /// Byte offset where a trailing `//` comment starts, or the line length.
    code_end: usize,
    /// Text of the block comments (or parts of them) on this line.
    block_comments: Vec<String>,
}

fn scan_line(line: &str, in_block: &mut bool) -> LineScan {
    let b = line.as_bytes();
    let mut i = 0;
    let mut comments = Vec::new();
    let mut current = String::new();
    let mut code_end = line.len();
    while i < b.len() {
        if *in_block {
            if b[i] == b'*' && b.get(i + 1) == Some(&b'/') {
                *in_block = false;
                comments.push(std::mem::take(&mut current));
                i += 2;
            } else {
                let ch_len = line[i..].chars().next().map_or(1, char::len_utf8);
                current.push_str(&line[i..i + ch_len]);
                i += ch_len;
            }
            continue;
        }
        match b[i] {
            b'/' if b.get(i + 1) == Some(&b'/') => {
                code_end = i;
                break;
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                *in_block = true;
                i += 2;
            }
            b'"' => {
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    i += 1;
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    if *in_block {
        comments.push(current);
    }
    LineScan {
        code_end: code_end.min(line.len()),
        block_comments: comments,
    }
}

/// `RENDERTARGETS: 0,1` or `DRAWBUFFERS:01` at the start of a block comment's text.
fn comment_directive(comment: &str) -> Option<Directive> {
    let text = comment.trim_start();
    if let Some(rest) = text.strip_prefix("RENDERTARGETS:") {
        let list: Option<Vec<u32>> = rest
            .trim()
            .split(',')
            .map(|s| s.trim().parse().ok())
            .collect();
        return list.filter(|l| !l.is_empty()).map(Directive::RenderTargets);
    }
    if let Some(rest) = text.strip_prefix("DRAWBUFFERS:") {
        let list: Option<Vec<u32>> = rest.trim().chars().map(|c| c.to_digit(10)).collect();
        return list.filter(|l| !l.is_empty()).map(Directive::DrawBuffers);
    }
    None
}

/// `<type> <name> = <value>;` after a leading `const `.
fn parse_const(rest: &str) -> Option<ConstDirective> {
    let (decl, value) = rest.split_once('=')?;
    let mut words = decl.split_whitespace();
    let ty = words.next()?;
    let name = words.next()?;
    if words.next().is_some() || !is_identifier(ty) || !is_identifier(name) {
        return None;
    }
    let value = value.split_once(';')?.0.trim();
    (!value.is_empty()).then(|| ConstDirective {
        ty: ty.to_string(),
        name: name.to_string(),
        value: value.to_string(),
    })
}

fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Runs pp-rs over the expanded text with `defines`, converting tokens and keeping only the
/// directives whose markers survived.
pub(crate) fn preprocess(
    expanded: Expanded,
    defines: &[(String, String)],
) -> Result<Preprocessed, Diagnostic> {
    let text = &expanded.text;
    let line_starts: Vec<usize> = std::iter::once(0)
        .chain(text.match_indices('\n').map(|(i, _)| i + 1))
        .collect();
    let line_of = |offset: u32| -> u32 {
        let offset = (offset as usize).min(text.len());
        line_starts.partition_point(|s| *s <= offset) as u32
    };
    let mut pp = Preprocessor::new(text);
    for (k, v) in defines {
        pp.add_define(k, v).map_err(|e| {
            Diagnostic::new(Phase::Preprocess, format!("cannot define `{k}`: {e:?}"))
        })?;
    }
    let mut out = Preprocessed {
        version: 110,
        profile: None,
        extensions: Vec::new(),
        tokens: Vec::new(),
        map: LineMap::default(),
        render_targets: None,
        consts: Vec::new(),
    };
    for item in pp {
        let tok = item.map_err(|(e, loc)| {
            Diagnostic::new(Phase::Preprocess, format!("{e:?}"))
                .at(expanded.map.locate(line_of(loc.start)))
        })?;
        let line = line_of(tok.location.start);
        let kind = match tok.value {
            TokenValue::Ident(s) => {
                if let Some(i) = s.strip_prefix(MARKER).and_then(|n| n.parse::<usize>().ok()) {
                    match expanded.directives.get(i) {
                        Some(Directive::RenderTargets(l) | Directive::DrawBuffers(l)) => {
                            out.render_targets = Some(l.clone());
                        }
                        Some(Directive::Const(c)) => out.consts.push(c.clone()),
                        None => {}
                    }
                    continue;
                }
                TokKind::Ident(s)
            }
            TokenValue::Integer(i) => TokKind::Int(if i.signed && i.value > i32::MAX as u64 {
                format!("0x{:X}", i.value)
            } else if i.signed {
                i.value.to_string()
            } else {
                format!("{}u", i.value)
            }),
            TokenValue::Float(f) => TokKind::Float(float_literal(f.value)),
            TokenValue::Punct(p) => TokKind::Punct(punct(p)),
            TokenValue::Version(v) => {
                for t in &v.tokens {
                    match &t.value {
                        TokenValue::Integer(i) => out.version = i.value as u32,
                        TokenValue::Ident(s) => out.profile = Some(s.clone()),
                        _ => {}
                    }
                }
                continue;
            }
            TokenValue::Extension(e) => {
                let words: Vec<String> = e
                    .tokens
                    .iter()
                    .map(|t| match &t.value {
                        TokenValue::Ident(s) => s.clone(),
                        TokenValue::Punct(p) => punct(*p).to_string(),
                        _ => String::new(),
                    })
                    .collect();
                out.extensions.push(words.join(" "));
                continue;
            }
            TokenValue::Pragma(_) => continue,
        };
        out.tokens.push(Tok { kind, line });
    }
    out.map = expanded.map;
    Ok(out)
}

/// GLSL spelling of a float literal; pp-rs literals are finite or overflow to infinity.
fn float_literal(v: f32) -> String {
    if !v.is_finite() {
        return "1e38".into();
    }
    let s = format!("{v:?}");
    if s.contains(['.', 'e']) {
        s
    } else {
        format!("{s}.0")
    }
}

fn punct(p: Punct) -> &'static str {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn run(files: &[(&str, &str)], entry: &str, defines: &[(&str, &str)]) -> Preprocessed {
        let source = PackSource::from_files(files.iter().copied());
        let expanded = expand(&source, entry, &[], &OptionValues::new()).expect("expands");
        let defines: Vec<(String, String)> = defines
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        preprocess(expanded, &defines).expect("preprocesses")
    }

    #[test]
    fn includes_resolve_absolute_and_relative_paths() {
        assert_eq!(
            resolve_include("world0/a.fsh", " \"/lib/x.glsl\""),
            "lib/x.glsl"
        );
        assert_eq!(
            resolve_include("world0/a.fsh", " \"../lib/x.glsl\""),
            "lib/x.glsl"
        );
        assert_eq!(
            resolve_include("program/a.glsl", " \"b.glsl\""),
            "program/b.glsl"
        );
    }

    #[test]
    fn only_directives_in_active_code_survive() {
        let src = "#version 450\n\
            #ifdef A\n/* RENDERTARGETS: 3 */\n#else\n/* RENDERTARGETS: 1,2 */\n#endif\n\
            /*\nconst int colortex1Format = RGBA16F;\n*/\n\
            #if 0\n/*\nconst int colortex2Format = R8;\n*/\n#endif\n\
            void main() {}\n";
        let pp = run(&[("a.fsh", src)], "a.fsh", &[]);
        assert_eq!(pp.render_targets, Some(vec![1, 2]));
        assert_eq!(pp.consts.len(), 1);
        assert_eq!(pp.consts[0].name, "colortex1Format");
        assert_eq!(pp.consts[0].value, "RGBA16F");
        let pp = run(&[("a.fsh", src)], "a.fsh", &[("A", "")]);
        assert_eq!(pp.render_targets, Some(vec![3]));
    }

    #[test]
    fn token_lines_map_back_through_includes() {
        let pp = run(
            &[
                (
                    "world0/a.fsh",
                    "#version 450\n#include \"/lib/b.glsl\"\nfloat y;\n",
                ),
                ("lib/b.glsl", "\nfloat x;\n"),
            ],
            "world0/a.fsh",
            &[],
        );
        let x = pp
            .tokens
            .iter()
            .find(|t| t.ident() == Some("x"))
            .expect("x");
        let y = pp
            .tokens
            .iter()
            .find(|t| t.ident() == Some("y"))
            .expect("y");
        let at = |t: &Tok| pp.map.locate(t.line).expect("mapped");
        assert_eq!(at(x).to_string(), "lib/b.glsl:2");
        assert_eq!(at(y).to_string(), "world0/a.fsh:3");
    }

    #[test]
    fn missing_include_reports_the_including_line() {
        let source = PackSource::from_files([("a.fsh", "\n#include \"/nope.glsl\"\n")]);
        let err = expand(&source, "a.fsh", &[], &OptionValues::new())
            .err()
            .expect("fails");
        assert_eq!(err.phase, Phase::Include);
        assert_eq!(err.location.expect("located").to_string(), "a.fsh:2");
    }
}
