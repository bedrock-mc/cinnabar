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
    };
    x.file(path, None)?;
    Ok(Expanded {
        text: x.out,
        map: x.map,
    })
}

/// One source file being expanded, with its own comment scope and line iterator.
struct FileFrame {
    path: String,
    file: u32,
    text: Arc<str>,
    cursor: usize,
    line: usize,
    pending: Option<(String, u32)>,
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
            pending: None,
        })
    }

    /// Expands includes in source order using heap frames, reporting cycles and malformed files.
    fn file(&mut self, path: &str, from: Option<SourceLocation>) -> Result<(), Diagnostic> {
        let mut frames = vec![self.frame(path, from)?];
        while let Some(frame) = frames.last_mut() {
            if let Some((comments, line)) = frame.pending.take() {
                self.append(&comments, frame.file, line);
                continue;
            }
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
            let logical = line.replace("\\\n", "");
            let scan = scan_line(&logical, &mut false);
            let undef = defined_name(scan.code.trim_start()).map(|name| format!("#undef {name}"));
            let comment_bytes = scan.comments.iter().map(|(_, text)| text.len() + 1).sum();
            // Include lines count too: duplicating empty files still spends traversal work.
            self.remaining = self
                .remaining
                .checked_sub(
                    line.len()
                        .saturating_add(1)
                        .max(comment_bytes)
                        .saturating_add(undef.as_ref().map_or(0, |text| text.len() + 1)),
                )
                .ok_or_else(|| {
                    Diagnostic::new(Phase::Include, "expanded GLSL exceeds byte limit")
                        .at(Some(here.clone()))
                })?;
            let code = &scan.code;
            if is_unknown_directive(code.trim_start()) {
                self.append("", frame.file, n as u32 + 1);
                continue;
            }
            if let Some(undef) = undef {
                self.append(&undef, frame.file, n as u32 + 1);
            }
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
                let file = frame.file;
                if frames.iter().any(|frame| frame.path == target) {
                    return Err(Diagnostic::new(
                        Phase::Include,
                        format!("include cycle through `{target}`"),
                    )
                    .at(Some(here)));
                }
                let filename_offset = code.len() - rest.trim_start().len();
                let split = scan
                    .comments
                    .partition_point(|(offset, _)| *offset < filename_offset);
                for (_, comment) in &scan.comments[..split] {
                    self.append(comment, file, n as u32 + 1);
                }
                let comments = scan.comments[split..]
                    .iter()
                    .map(|(_, text)| text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                if !comments.is_empty() {
                    frames.last_mut().unwrap().pending = Some((comments, n as u32 + 1));
                }
                frames.push(self.frame(&target, Some(here))?);
            } else {
                self.append(line, frame.file, n as u32 + 1);
            }
        }
        Ok(())
    }

    /// Appends expanded text and maps each physical line back to its source file.
    fn append(&mut self, text: &str, file: u32, line: u32) {
        self.out.push_str(text);
        self.out.push('\n');
        self.map
            .lines
            .extend((0..text.split('\n').count()).map(|index| (file, line + index as u32)));
    }
}

/// The macro name a `#define` line defines.
fn defined_name(trimmed: &str) -> Option<&str> {
    let rest = trimmed.strip_prefix('#')?.trim_start();
    let rest = rest.strip_prefix("define")?;
    if !rest.starts_with([' ', '\t']) {
        return None;
    }
    let rest = rest.trim_start();
    let end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(rest.len());
    (end > 0).then(|| &rest[..end])
}

/// A `#` line whose directive name GLSL doesn't define. A lone `#` is the null directive.
fn is_unknown_directive(trimmed: &str) -> bool {
    let Some(rest) = trimmed.strip_prefix('#') else {
        return false;
    };
    let rest = rest.trim_start();
    let end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(rest.len());
    let word = &rest[..end];
    if word.is_empty() {
        return !rest.is_empty() && !rest.starts_with("//") && !rest.starts_with("/*");
    }
    word != "include" && !Preprocessor::is_directive(word)
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
    /// Source with comment bytes replaced by spaces, preserving offsets and newlines.
    code: String,
    /// Comments in source order, including delimiters and their byte offsets.
    comments: Vec<(usize, String)>,
}

/// Separates comments from code without interpreting preprocessing conditionals.
fn scan_line(text: &str, in_block: &mut bool) -> LineScan {
    let mut code = String::with_capacity(text.len());
    let mut comments = Vec::new();
    let mut i = 0;
    while i < text.len() {
        let start = i;
        if *in_block || text[i..].starts_with("/*") {
            let body = i + if *in_block { 0 } else { 2 };
            let end = text[body..].find("*/").map(|end| body + end + 2);
            *in_block = end.is_none();
            i = end.unwrap_or(text.len());
        } else if text[i..].starts_with("//") {
            i = text[i..].find('\n').map_or(text.len(), |end| i + end);
        } else if text[i..].starts_with('"') {
            i += 1;
            let mut escaped = false;
            while i < text.len() {
                let ch = text[i..].chars().next().unwrap();
                i += ch.len_utf8();
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    break;
                }
            }
            code.push_str(&text[start..i]);
            continue;
        } else {
            let ch = text[i..].chars().next().unwrap();
            code.push(ch);
            i += ch.len_utf8();
            continue;
        }
        comments.push((start, text[start..i].to_string()));
        code.extend(
            text[start..i]
                .bytes()
                .map(|byte| if byte == b'\n' { '\n' } else { ' ' }),
        );
    }
    LineScan { code, comments }
}

/// Reads metadata from active code and comments, preserving their source order.
fn metadata(text: &str, remaining: &mut usize) -> Result<Vec<Directive>, Diagnostic> {
    let scan = scan_line(text, &mut false);
    let mut found = Vec::new();
    let mut keep = |offset, directive: Directive| -> Result<(), Diagnostic> {
        let bytes = size_of::<Directive>()
            + match &directive {
                Directive::RenderTargets(values) | Directive::DrawBuffers(values) => {
                    values.len() * size_of::<u32>()
                }
                Directive::Const(value) => value.ty.len() + value.name.len() + value.value.len(),
            };
        *remaining = remaining.checked_sub(bytes).ok_or_else(storage_limit)?;
        found.push((offset, directive));
        Ok(())
    };
    for (offset, comment) in &scan.comments {
        let body = comment
            .strip_prefix("/*")
            .and_then(|text| text.strip_suffix("*/"))
            .or_else(|| comment.strip_prefix("//"))
            .unwrap_or(comment);
        if let Some(directive) = comment_directive(body) {
            keep(*offset, directive)?;
        }
        for line in body.lines() {
            if let Some(value) = line
                .trim_start()
                .strip_prefix("const")
                .filter(|rest| rest.starts_with(char::is_whitespace))
                .and_then(parse_const)
            {
                keep(*offset, Directive::Const(value))?;
            }
        }
    }
    let mut offset = 0;
    for line in scan.code.split_inclusive('\n') {
        if let Some(value) = line
            .trim_start()
            .strip_prefix("const")
            .filter(|rest| rest.starts_with(char::is_whitespace))
            .and_then(parse_const)
        {
            keep(
                offset + line.len() - line.trim_start().len(),
                Directive::Const(value),
            )?;
        }
        offset += line.len();
    }
    found.sort_by_key(|(offset, _)| *offset);
    Ok(found.into_iter().map(|(_, directive)| directive).collect())
}

/// Reports a recoverable failure before retaining more preprocessing output.
fn storage_limit() -> Diagnostic {
    Diagnostic::new(
        Phase::Preprocess,
        "GLSL preprocessing exceeds storage limit",
    )
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

/// Runs pp-rs over expanded text, preserving token spelling and reading active metadata.
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
    pp.set_iris_conditions(true);
    pp.set_preserve_glsl_text();
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
    let mut active = String::new();
    while let Some(item) = pp.next_text() {
        let token = item.map_err(|(error, location)| {
            Diagnostic::new(
                Phase::Preprocess,
                if pp.work_remaining() == 0 {
                    "GLSL preprocessing exceeds work limit".into()
                } else {
                    format!("{error:?}")
                },
            )
            .at(expanded.map.locate(line_of(location.start)))
        })?;
        let line = line_of(token.source_offset);
        retained = retained
            .checked_sub(token.text.len())
            .ok_or_else(|| storage_limit().at(expanded.map.locate(line)))?;
        active.push_str(&token.text);
        let Some(value) = token.value else {
            continue;
        };
        let kind = match value {
            TokenValue::Ident(name) => TokKind::Ident(name),
            TokenValue::Integer(_) => TokKind::Int(token.text.trim_start().to_string()),
            TokenValue::Float(_) => TokKind::Float(token.text.trim_start().to_string()),
            TokenValue::Punct(value) => TokKind::Punct(punct(value)),
            TokenValue::Version(value) => {
                for token in value.tokens {
                    match token.value {
                        TokenValue::Integer(value) => out.version = value.value as u32,
                        TokenValue::Ident(name) => out.profile = Some(name),
                        _ => {}
                    }
                }
                add_define(&mut pp, "__VERSION__", &out.version.to_string())
                    .map_err(|error| error.at(expanded.map.locate(line)))?;
                continue;
            }
            TokenValue::Extension(value) => {
                let words: Vec<_> = value
                    .tokens
                    .into_iter()
                    .map(|token| match token.value {
                        TokenValue::Ident(name) => name,
                        TokenValue::Punct(value) => punct(value).to_string(),
                        _ => String::new(),
                    })
                    .collect();
                let extension = words.join(" ");
                retained = retained
                    .checked_sub(size_of::<String>() + extension.len())
                    .ok_or_else(storage_limit)?;
                out.extensions.push(extension);
                continue;
            }
            TokenValue::Pragma(_) => continue,
        };
        let bytes = size_of::<Tok>()
            + match &kind {
                TokKind::Ident(text) | TokKind::Int(text) | TokKind::Float(text) => text.len(),
                TokKind::Punct(_) => 0,
            };
        retained = retained
            .checked_sub(bytes)
            .ok_or_else(|| storage_limit().at(expanded.map.locate(line)))?;
        out.tokens.push(Tok { kind, line });
    }
    for directive in metadata(&active, &mut retained)? {
        match directive {
            Directive::RenderTargets(values) | Directive::DrawBuffers(values) => {
                out.render_targets = Some(values)
            }
            Directive::Const(value) => out.consts.push(value),
        }
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

#[cfg(test)]
mod tests;
