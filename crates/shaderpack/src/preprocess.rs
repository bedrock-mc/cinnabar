//! Iris-style `#include` expansion, option overrides, comment directives and full macro and
//! conditional expansion with pp-rs (the preprocessor naga's GLSL frontend uses).

use crate::diagnostics::{Diagnostic, Phase, SourceLocation};
use crate::options::{OptionValues, PackOption, rewrite_option_line};
use crate::source::{MAX_TEXT_FILE_BYTES, PackSource, normalize_newlines};
use crate::token::{Tok, TokKind, punct};
use pp_rs::pp::Preprocessor;
use pp_rs::token::TokenValue;
use std::collections::HashMap;
use std::sync::Arc;

/// Maximum expanded source or retained preprocessing storage for one GLSL stage.
const MAX_EXPANDED_GLSL_BYTES: usize = 4 * MAX_TEXT_FILE_BYTES;
/// Macro work includes tokens evaluated only in conditions and discarded expansions.
const MAX_GLSL_PREPROCESSING_TOKENS: u64 = 4 * MAX_TEXT_FILE_BYTES as u64;

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

    /// Interns a file path and returns its stable line-map index.
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
    normalized: HashMap<String, Arc<str>>,
    remaining: usize,
    options: &'a [PackOption],
    values: &'a OptionValues,
    out: String,
    map: LineMap,
    directives: Vec<Directive>,
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
    expand_limited(source, path, options, values, MAX_EXPANDED_GLSL_BYTES)
}

/// Expands one stage within the supplied include-processing byte budget.
fn expand_limited(
    source: &PackSource,
    path: &str,
    options: &[PackOption],
    values: &OptionValues,
    limit: usize,
) -> Result<Expanded, Diagnostic> {
    let mut x = Expander {
        source,
        normalized: HashMap::new(),
        remaining: limit,
        options,
        values,
        out: String::new(),
        map: LineMap::default(),
        directives: Vec::new(),
    };
    x.file(path, None)?;
    Ok(Expanded {
        text: x.out,
        map: x.map,
        directives: x.directives,
    })
}

/// One source file being expanded, with its own comment scope and line iterator.
struct FileFrame {
    path: String,
    file: u32,
    text: Arc<str>,
    cursor: usize,
    line: usize,
}

impl FileFrame {
    /// Takes the next normalized physical line without copying the file for each include.
    fn next_line(&mut self) -> Option<(usize, &str)> {
        if self.cursor == self.text.len() {
            return None;
        }
        let start = self.cursor;
        let end = self.text[start..]
            .find('\n')
            .map_or(self.text.len(), |end| start + end);
        self.cursor = end + usize::from(end < self.text.len());
        let line = self.line;
        self.line += 1;
        Some((line, &self.text[start..end]))
    }
}

impl<'a> Expander<'a> {
    /// Normalizes a file once and opens an include frame; errors retain the caller location.
    fn frame(&mut self, path: &str, from: Option<SourceLocation>) -> Result<FileFrame, Diagnostic> {
        let source = self.source;
        let text = source.text(path).ok_or_else(|| {
            Diagnostic::new(Phase::Include, format!("cannot find `{path}`")).at(from)
        })?;
        let text = self
            .normalized
            .entry(path.to_string())
            .or_insert_with(|| Arc::from(normalize_newlines(text).as_ref()))
            .clone();
        Ok(FileFrame {
            path: path.to_string(),
            file: self.map.file_index(path),
            text,
            cursor: 0,
            line: 0,
        })
    }

    /// Expands includes in source order using heap frames, reporting cycles and malformed files.
    fn file(&mut self, path: &str, from: Option<SourceLocation>) -> Result<(), Diagnostic> {
        let mut frames = vec![self.frame(path, from)?];
        while let Some(frame) = frames.last_mut() {
            let Some((n, first)) = frame.next_line() else {
                frames.pop();
                continue;
            };
            let mut line = first.to_string();
            let mut logical = first.to_string();
            let mut in_block = false;
            loop {
                let continued = logical.ends_with('\\');
                if !continued {
                    scan_line(&logical, &mut in_block);
                    if !in_block {
                        break;
                    }
                }
                let Some((_, next)) = frame.next_line() else {
                    if in_block {
                        return Err(Diagnostic::new(
                            Phase::Preprocess,
                            "unterminated block comment",
                        )
                        .at(Some(SourceLocation {
                            file: frame.path.clone(),
                            line: n as u32 + 1,
                        })));
                    }
                    break;
                };
                line.push('\n');
                line.push_str(next);
                if continued {
                    logical.pop();
                    logical.push_str(next);
                } else {
                    logical = next.to_string();
                }
            }
            let line = line.as_str();
            let here = SourceLocation {
                file: frame.path.clone(),
                line: n as u32 + 1,
            };
            let rewritten = if self.values.is_empty() {
                None
            } else {
                rewrite_option_line(line, self.options, self.values)
            };
            let line = rewritten.as_deref().unwrap_or(line);
            // pp-rs tokenizes skipped groups and rejects quotes in #error messages.
            let line = if line.trim_start().starts_with("#error") {
                line.replace('"', "")
            } else {
                line.to_string()
            };
            let logical = line.replace("\\\n", "");
            let code = scan_line(&logical, &mut false).code;
            let (out, marker_before) = self.directives_in(&logical, &line);
            // Include lines count too: duplicating empty files still spends traversal work.
            self.remaining = self
                .remaining
                .checked_sub(line.len().max(out.len()).saturating_add(1))
                .ok_or_else(|| {
                    Diagnostic::new(Phase::Include, "expanded GLSL exceeds byte limit")
                        .at(Some(here.clone()))
                })?;
            let directive = code.trim_start().strip_prefix('#').map(str::trim_start);
            if let Some(rest) = directive
                .and_then(|d| d.strip_prefix("include"))
                .filter(|rest| {
                    rest.is_empty()
                        || rest.starts_with(|c: char| c.is_whitespace() || c == '"' || c == '<')
                })
            {
                let target = resolve_include(&frame.path, rest).map_err(|message| {
                    Diagnostic::new(Phase::Include, message).at(Some(here.clone()))
                })?;
                if frames.iter().any(|frame| frame.path == target) {
                    return Err(Diagnostic::new(
                        Phase::Include,
                        format!("include cycle through `{target}`"),
                    )
                    .at(Some(here)));
                }
                frames.push(self.frame(&target, Some(here))?);
            } else {
                self.out.push_str(&out);
                self.out.push('\n');
                let count = line.lines().count().max(1);
                self.map.lines.extend((0..out.split('\n').count()).map(|j| {
                    let original = if marker_before {
                        j.saturating_sub(1)
                    } else {
                        j
                    };
                    (frame.file, (n + original.min(count - 1) + 1) as u32)
                }));
            }
        }
        Ok(())
    }

    /// Keeps source text intact and emits metadata markers in the same conditional group.
    fn directives_in(&mut self, line: &str, original: &str) -> (String, bool) {
        let scan = scan_line(line, &mut false);
        let mut found = Vec::new();
        for comment in &scan.block_comments {
            if let Some(d) = comment_directive(comment) {
                found.push(d);
            }
        }
        for line in line.lines() {
            if let Some(c) = line
                .trim_start()
                .strip_prefix("const")
                .filter(|s| s.starts_with(char::is_whitespace))
                .and_then(parse_const)
            {
                found.push(Directive::Const(c));
            }
        }
        let mut markers = String::new();
        for directive in found {
            let i = self.directives.len();
            self.directives.push(directive);
            markers.push_str(&format!(" {MARKER}{i} "));
        }
        let control = scan
            .code
            .trim_start()
            .strip_prefix('#')
            .map(str::trim_start);
        let closes_group = control.is_some_and(|s| {
            ["endif", "else", "elif"].iter().any(|name| {
                s.strip_prefix(name)
                    .is_some_and(|tail| tail.is_empty() || tail.starts_with(char::is_whitespace))
            })
        });
        let mut out = original.to_string();
        if !markers.is_empty() {
            if closes_group {
                out = format!("{markers}\n{out}");
            } else {
                out.push('\n');
                out.push_str(&markers);
            }
        }
        (out, closes_group && !markers.is_empty())
    }
}

/// `shaders/`-relative target of an `#include` argument written in `from`.
fn resolve_include(from: &str, arg: &str) -> Result<String, String> {
    let arg = arg.trim();
    let (tail, close) = if let Some(tail) = arg.strip_prefix('"') {
        (tail, '"')
    } else if let Some(tail) = arg.strip_prefix('<') {
        (tail, '>')
    } else {
        return Err("include requires a delimited filename".into());
    };
    let end = tail.find(close).ok_or("unterminated include filename")?;
    let name = &tail[..end];
    if name.is_empty() || !tail[end + close.len_utf8()..].trim().is_empty() {
        return Err("include requires one non-empty filename".into());
    }
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
    Ok(parts.join("/"))
}

struct LineScan {
    /// Source code with comments replaced by whitespace.
    code: String,
    /// Text of the block comments (or parts of them) on this line.
    block_comments: Vec<String>,
}

/// Replaces comments with whitespace while collecting their text and tracking multiline scope.
fn scan_line(line: &str, in_block: &mut bool) -> LineScan {
    let b = line.as_bytes();
    let mut i = 0;
    let mut comments = Vec::new();
    let mut current = String::new();
    let mut code = String::new();
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
                break;
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                *in_block = true;
                code.push(' ');
                i += 2;
            }
            b'"' => {
                let start = i;
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    i += 1;
                }
                i = (i + 1).min(b.len());
                code.push_str(&line[start..i]);
            }
            _ => {
                let ch = line[i..].chars().next().expect("cursor is within the line");
                code.push(ch);
                i += ch.len_utf8();
            }
        }
    }
    if *in_block {
        comments.push(current);
    }
    LineScan {
        code,
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
        return list.map(Directive::DrawBuffers);
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

/// Checks whether text is a non-empty ASCII identifier.
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
    preprocess_limited(
        expanded,
        defines,
        MAX_GLSL_PREPROCESSING_TOKENS,
        MAX_EXPANDED_GLSL_BYTES,
    )
}

/// Preprocesses a stage within work and retained-storage budgets.
fn preprocess_limited(
    expanded: Expanded,
    defines: &[(String, String)],
    work: u64,
    mut retained: usize,
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
    pp.set_work_limit(work);
    pp.set_source_lines(expanded.map.lines.iter().map(|(_, line)| *line).collect());
    for (k, v) in defines {
        add_define(&mut pp, k, v)?;
    }
    add_define(&mut pp, "__VERSION__", "110")?;
    let mut out = Preprocessed {
        version: 110,
        profile: None,
        extensions: Vec::new(),
        tokens: Vec::new(),
        map: LineMap::default(),
        render_targets: None,
        consts: Vec::new(),
    };
    while let Some(item) = pp.next() {
        let tok = item.map_err(|(e, loc)| {
            Diagnostic::new(
                Phase::Preprocess,
                if pp.work_remaining() == 0 {
                    "GLSL preprocessing exceeds work limit".into()
                } else {
                    format!("{e:?}")
                },
            )
            .at(expanded.map.locate(line_of(loc.start)))
        })?;
        let line = line_of(tok.location.start);
        retained = retained
            .checked_sub(retained_bytes(&tok.value, &expanded.directives))
            .ok_or_else(|| {
                Diagnostic::new(
                    Phase::Preprocess,
                    "GLSL preprocessing exceeds storage limit",
                )
                .at(expanded.map.locate(line))
            })?;
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
                add_define(&mut pp, "__VERSION__", &out.version.to_string())
                    .map_err(|diagnostic| diagnostic.at(expanded.map.locate(line)))?;
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

/// Adds a macro, reporting invalid definitions and exhausted work as recoverable diagnostics.
fn add_define(pp: &mut Preprocessor<'_>, name: &str, value: &str) -> Result<(), Diagnostic> {
    pp.add_define(name, value).map_err(|error| {
        Diagnostic::new(
            Phase::Preprocess,
            if pp.work_remaining() == 0 {
                "GLSL preprocessing exceeds work limit".to_string()
            } else {
                format!("cannot define `{name}`: {error:?}")
            },
        )
    })
}

/// Bounds retained tokens and metadata copies, including markers duplicated by macros.
fn retained_bytes(token: &TokenValue, directives: &[Directive]) -> usize {
    let text = match token {
        TokenValue::Ident(name) => {
            let metadata = name
                .strip_prefix(MARKER)
                .and_then(|index| index.parse::<usize>().ok())
                .and_then(|index| directives.get(index));
            name.len()
                + match metadata {
                    Some(Directive::RenderTargets(values) | Directive::DrawBuffers(values)) => {
                        values.len() * size_of::<u32>()
                    }
                    Some(Directive::Const(value)) => {
                        size_of::<ConstDirective>()
                            + value.ty.len()
                            + value.name.len()
                            + value.value.len()
                    }
                    None => 0,
                }
        }
        TokenValue::Integer(_) | TokenValue::Float(_) => 32,
        TokenValue::Version(value) => value
            .tokens
            .iter()
            .map(|token| retained_bytes(&token.value, &[]))
            .sum(),
        TokenValue::Extension(value) => value
            .tokens
            .iter()
            .map(|token| retained_bytes(&token.value, &[]))
            .sum(),
        TokenValue::Pragma(value) => value
            .tokens
            .iter()
            .map(|token| retained_bytes(&token.value, &[]))
            .sum(),
        TokenValue::Punct(_) => 0,
    };
    size_of::<Tok>() + text
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

#[cfg(test)]
mod tests;
