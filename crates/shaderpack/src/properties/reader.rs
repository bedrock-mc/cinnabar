//! Line-level reading of `shaders.properties`: the conditional preprocessor and the
//! java.util.Properties key/value syntax, in one pass so that continuation lines are
//! never mistaken for directives.

use std::collections::BTreeMap;

use super::PropertiesDiagnostic;
use crate::expr::{Expr, macro_value};

/// One key/value pair from an active logical line.
pub(super) struct RawEntry {
    /// 1-based line where the logical line starts.
    pub(super) line: u32,
    pub(super) key: String,
    pub(super) value: String,
}

/// Reads the key/value pairs of `text` that survive conditional preprocessing, in file
/// order. Malformed directives are reported to `diagnostics` and reading continues.
pub(super) fn read_entries(
    text: &str,
    defines: &BTreeMap<String, String>,
    diagnostics: &mut Vec<PropertiesDiagnostic>,
) -> Vec<RawEntry> {
    let mut conditionals = Conditionals {
        defines: defines.clone(),
        frames: Vec::new(),
    };
    let mut entries = Vec::new();
    // Logical line being joined from continuation lines: (first line, text so far).
    let mut pending: Option<(u32, String)> = None;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    for (index, physical) in text.lines().enumerate() {
        let line = u32::try_from(index + 1).unwrap_or(u32::MAX);
        if let Some((start, mut logical)) = pending.take() {
            let piece = physical.trim_start();
            match strip_continuation(piece) {
                Some(head) => {
                    logical.push_str(head);
                    pending = Some((start, logical));
                }
                None => {
                    logical.push_str(piece);
                    entries.push(parse_entry(start, &logical));
                }
            }
            continue;
        }
        let trimmed = physical.trim_start();
        if let Some(rest) = trimmed.strip_prefix('#') {
            if let Some(directive) = Directive::parse(rest) {
                conditionals.apply(directive, line, diagnostics);
            }
            continue;
        }
        if !conditionals.active() || trimmed.is_empty() || trimmed.starts_with('!') {
            continue;
        }
        match strip_continuation(trimmed) {
            Some(head) => pending = Some((line, head.to_owned())),
            None => entries.push(parse_entry(line, trimmed)),
        }
    }
    if let Some((start, logical)) = pending {
        entries.push(parse_entry(start, &logical));
    }
    conditionals.finish(diagnostics);
    entries
}

/// `text` without its continuation backslash when it ends in an odd number of `\`.
fn strip_continuation(text: &str) -> Option<&str> {
    let backslashes = text.bytes().rev().take_while(|&b| b == b'\\').count();
    if backslashes.is_multiple_of(2) {
        None
    } else {
        Some(&text[..text.len() - 1])
    }
}

/// Splits a logical line into key and value: the key ends at the first unescaped `=`,
/// `:` or whitespace; whitespace and one `=`/`:` separator follow; both sides are
/// unescaped and trimmed.
fn parse_entry(line: u32, logical: &str) -> RawEntry {
    let mut key_end = logical.len();
    let mut escaped = false;
    for (index, c) in logical.char_indices() {
        if escaped {
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == '=' || c == ':' || c.is_whitespace() {
            key_end = index;
            break;
        }
    }
    let mut value = logical[key_end..].trim_start();
    if let Some(stripped) = value.strip_prefix(['=', ':']) {
        value = stripped.trim_start();
    }
    RawEntry {
        line,
        key: unescape(&logical[..key_end]).trim().to_owned(),
        value: unescape(value).trim().to_owned(),
    }
}

/// Java properties escapes: `\t \n \r \f`, `\uXXXX`, and `\c` → `c` for anything else.
fn unescape(text: &str) -> String {
    if !text.contains('\\') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('f') => out.push('\u{c}'),
            Some('u') => {
                let rest = chars.as_str();
                if let Some(hex) = rest.get(..4)
                    && let Ok(code) = u32::from_str_radix(hex, 16)
                    && let Some(decoded) = char::from_u32(code)
                {
                    out.push(decoded);
                    chars = rest[4..].chars();
                } else {
                    out.push('u');
                }
            }
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// A preprocessor directive; arguments are trimmed and stripped of `//` comments.
#[derive(Clone, Copy)]
enum Directive<'a> {
    If(&'a str),
    Ifdef(&'a str),
    Ifndef(&'a str),
    Elif(&'a str),
    Else,
    Endif,
    Define(&'a str),
    Undef(&'a str),
}

impl<'a> Directive<'a> {
    /// Parses the text after a line's `#`; `None` when the line is a plain comment
    /// (including commented-out keys such as `#uniform.float.x=...`).
    fn parse(rest: &'a str) -> Option<Self> {
        let rest = rest.trim_start();
        let word_len = rest.bytes().take_while(u8::is_ascii_alphabetic).count();
        let (word, args) = rest.split_at(word_len);
        if args
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.')
        {
            return None;
        }
        let args = args.split_once("//").map_or(args, |(code, _)| code).trim();
        Some(match word {
            "if" => Directive::If(args),
            "ifdef" => Directive::Ifdef(args),
            "ifndef" => Directive::Ifndef(args),
            "elif" => Directive::Elif(args),
            "else" => Directive::Else,
            "endif" => Directive::Endif,
            "define" => Directive::Define(args),
            "undef" => Directive::Undef(args),
            _ => return None,
        })
    }
}

/// One open `#if`/`#ifdef`/`#ifndef` block.
struct Frame {
    /// Line of the opening directive, for unterminated-block diagnostics.
    line: u32,
    /// Whether the current branch is active.
    active: bool,
    /// Whether a branch was already taken, or the block sits in an inactive region, so
    /// later `#elif`/`#else` branches stay inactive.
    taken: bool,
    seen_else: bool,
}

/// Conditional-compilation state: the macro table and the open blocks.
struct Conditionals {
    defines: BTreeMap<String, String>,
    frames: Vec<Frame>,
}

impl Conditionals {
    fn active(&self) -> bool {
        self.frames.last().is_none_or(|frame| frame.active)
    }

    fn open(&mut self, line: u32, parent_active: bool, active: bool) {
        self.frames.push(Frame {
            line,
            active,
            taken: active || !parent_active,
            seen_else: false,
        });
    }

    fn apply(
        &mut self,
        directive: Directive<'_>,
        line: u32,
        diagnostics: &mut Vec<PropertiesDiagnostic>,
    ) {
        let mut report = |message: &str| {
            diagnostics.push(PropertiesDiagnostic {
                line,
                message: message.to_owned(),
            });
        };
        match directive {
            Directive::If(text) => {
                let parent_active = self.active();
                let active = parent_active && self.test(text, &mut report);
                self.open(line, parent_active, active);
            }
            Directive::Ifdef(text) | Directive::Ifndef(text) => {
                let negate = matches!(directive, Directive::Ifndef(_));
                let parent_active = self.active();
                let active = parent_active
                    && match text.split_whitespace().next() {
                        Some(name) => self.defines.contains_key(name) != negate,
                        None => {
                            report("conditional directive without a macro name");
                            false
                        }
                    };
                self.open(line, parent_active, active);
            }
            Directive::Elif(text) => {
                let Some(frame) = self.frames.last() else {
                    report("#elif without matching #if");
                    return;
                };
                if frame.seen_else {
                    report("#elif after #else");
                }
                let pending = !frame.taken && !frame.seen_else;
                let active = pending && self.test(text, &mut report);
                if let Some(frame) = self.frames.last_mut() {
                    frame.active = active;
                    frame.taken |= active;
                }
            }
            Directive::Else => {
                let Some(frame) = self.frames.last_mut() else {
                    report("#else without matching #if");
                    return;
                };
                if frame.seen_else {
                    report("duplicate #else");
                }
                frame.active = !frame.taken && !frame.seen_else;
                frame.taken = true;
                frame.seen_else = true;
            }
            Directive::Endif => {
                if self.frames.pop().is_none() {
                    report("#endif without matching #if");
                }
            }
            Directive::Define(text) => {
                if self.active() {
                    match macro_name(text) {
                        Some((name, value)) => {
                            self.defines.insert(name.to_owned(), value.to_owned());
                        }
                        None => report("#define without a macro name"),
                    }
                }
            }
            Directive::Undef(text) => {
                if self.active() {
                    match macro_name(text) {
                        Some((name, _)) => {
                            self.defines.remove(name);
                        }
                        None => report("#undef without a macro name"),
                    }
                }
            }
        }
    }

    /// Evaluates an `#if`/`#elif` condition; malformed conditions are reported and false.
    fn test(&self, text: &str, report: &mut impl FnMut(&str)) -> bool {
        let expr = match Expr::parse(text) {
            Ok(expr) => expr,
            Err(err) => {
                report(&format!("invalid condition `{text}`: {err}"));
                return false;
            }
        };
        let lookup = |name: &str| self.defines.get(name).map(|value| macro_value(value));
        match expr.eval(&lookup) {
            Ok(value) => value != 0.0,
            Err(err) => {
                report(&format!("cannot evaluate condition `{text}`: {err}"));
                false
            }
        }
    }

    /// Reports every block still open at end of file.
    fn finish(self, diagnostics: &mut Vec<PropertiesDiagnostic>) {
        for frame in self.frames {
            diagnostics.push(PropertiesDiagnostic {
                line: frame.line,
                message: "conditional block is missing its #endif".to_owned(),
            });
        }
    }
}

/// Splits `#define`/`#undef` arguments into the macro name and its (trimmed) value.
fn macro_name(args: &str) -> Option<(&str, &str)> {
    let name_len = args
        .bytes()
        .take_while(|&b| b.is_ascii_alphanumeric() || b == b'_')
        .count();
    let (name, value) = args.split_at(name_len);
    if name.bytes().next().is_some_and(|b| !b.is_ascii_digit()) {
        Some((name, value.trim()))
    } else {
        None
    }
}
