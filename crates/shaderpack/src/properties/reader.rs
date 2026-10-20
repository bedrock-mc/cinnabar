//! Line-level reading of `shaders.properties` in Iris's two stages: a line-by-line
//! conditional preprocessor, then java.util.Properties over the surviving text. Directives
//! are recognised on every physical line, so a continuation never joins one.

use std::collections::BTreeMap;

use super::PropertiesDiagnostic;
use crate::source::MAX_EXPANDED_PROPERTIES_BYTES;
use pp_rs::pp::Preprocessor;
use pp_rs::token::TokenValue;

/// One key/value pair from an active logical line.
#[derive(Clone)]
pub(crate) struct RawEntry {
    /// 1-based line where the logical line starts.
    pub(crate) line: u32,
    pub(crate) key: String,
    pub(crate) value: String,
}

/// Reads the key/value pairs of `text` that survive conditional preprocessing, in file
/// order. Malformed directives are reported to `diagnostics` and reading continues.
pub(crate) fn read_entries(
    text: &str,
    defines: &BTreeMap<String, String>,
    diagnostics: &mut Vec<PropertiesDiagnostic>,
) -> Vec<RawEntry> {
    let mut conditionals = Conditionals {
        defines: defines.clone(),
        frames: Vec::new(),
    };
    // Preprocessed lines: blank lines are dropped, while directives, comments and inactive
    // lines leave an empty line that ends any continuation.
    let mut lines: Vec<(u32, String)> = Vec::new();
    let mut remaining_bytes = MAX_EXPANDED_PROPERTIES_BYTES;
    let mut raw_continuation = None;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut physical_lines = text.lines().enumerate();
    while let Some((index, physical)) = physical_lines.next() {
        let line = u32::try_from(index + 1).unwrap_or(u32::MAX);
        let trimmed = physical.trim();
        if trimmed.is_empty() {
            continue;
        }
        let continuation = raw_continuation.take();
        let kept = if let Some(rest) = trimmed.strip_prefix('#') {
            if Directive::parse(rest).is_none() {
                lines.push((line, String::new()));
                continue;
            }
            let mut in_comment = false;
            let mut code = directive_code(rest, &mut in_comment);
            while in_comment {
                let Some((_, physical)) = physical_lines.next() else {
                    break;
                };
                code.push(' ');
                code.push_str(&directive_code(physical, &mut in_comment));
            }
            if in_comment {
                diagnostics.push(PropertiesDiagnostic {
                    line,
                    message: "directive comment is missing its */".to_owned(),
                });
            } else if let Some(directive) = Directive::parse(&code) {
                conditionals.apply(directive, line, diagnostics);
            }
            String::new()
        } else if conditionals.active() {
            let keep_raw = continuation.unwrap_or_else(|| {
                let key = parse_entry(line, trimmed).key;
                matches!(key.as_str(), "screen" | "sliders")
                    || key.starts_with("screen.")
                    || key.starts_with("profile.")
            });
            let expanded = if keep_raw {
                let mut out = String::new();
                append_expanded(&mut out, trimmed, remaining_bytes).map(|()| out)
            } else {
                // Iris strips `#` from content before preprocessing other properties.
                expand(
                    &trimmed.replace('#', ""),
                    &conditionals.defines,
                    false,
                    remaining_bytes,
                )
            };
            match expanded {
                Ok(kept) => {
                    remaining_bytes -= kept.len();
                    raw_continuation = (!kept.starts_with('!')
                        && strip_continuation(&kept).is_some())
                    .then_some(keep_raw);
                    kept
                }
                Err(message) => {
                    diagnostics.push(PropertiesDiagnostic {
                        line,
                        message: message.to_owned(),
                    });
                    String::new()
                }
            }
        } else {
            String::new()
        };
        lines.push((line, kept));
    }
    conditionals.finish(diagnostics);

    let mut entries = Vec::new();
    // Logical line being joined from continuation lines: (first line, text so far).
    let mut pending: Option<(u32, String)> = None;
    for (line, text) in &lines {
        if let Some((start, mut logical)) = pending.take() {
            match strip_continuation(text) {
                Some(head) => {
                    logical.push_str(head);
                    pending = Some((start, logical));
                }
                None => {
                    logical.push_str(text);
                    entries.push(parse_entry(start, &logical));
                }
            }
            continue;
        }
        if text.is_empty() || text.starts_with('!') {
            continue;
        }
        match strip_continuation(text) {
            Some(head) => pending = Some((*line, head.to_owned())),
            None => entries.push(parse_entry(*line, text)),
        }
    }
    if let Some((start, logical)) = pending {
        entries.push(parse_entry(start, &logical));
    }
    entries
}

/// Reads `text` as plain java.util.Properties, without any preprocessing: lines starting
/// with `#` or `!` are comments (so `#if` blocks are ignored), a trailing odd backslash
/// continues the line, and an empty line ends a continuation. Iris reads the option-menu
/// layout (`screen`, `sliders`, `profile.*`), the pack's `.lang` files and the saved option
/// file this way. Entries are returned in file order; a repeated key appears again.
pub(crate) fn read_plain_entries(text: &str) -> Vec<RawEntry> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut entries = Vec::new();
    let mut pending: Option<(u32, String)> = None;
    for (index, physical) in text.lines().enumerate() {
        let line = u32::try_from(index + 1).unwrap_or(u32::MAX);
        let text = physical.trim_start_matches(property_space);
        if let Some((start, mut logical)) = pending.take() {
            match strip_continuation(text) {
                Some(head) => {
                    logical.push_str(head);
                    pending = Some((start, logical));
                }
                None => {
                    logical.push_str(text);
                    entries.push(parse_entry(start, &logical));
                }
            }
            continue;
        }
        if text.is_empty() || text.starts_with(['#', '!']) {
            continue;
        }
        match strip_continuation(text) {
            Some(head) => pending = Some((line, head.to_owned())),
            None => entries.push(parse_entry(line, text)),
        }
    }
    if let Some((start, logical)) = pending {
        entries.push(parse_entry(start, &logical));
    }
    entries
}

/// Iris defines flag macros (enabled boolean options, valueless environment macros) as `1`.
fn flag_value(value: &str) -> &str {
    if value.is_empty() { "1" } else { value }
}

/// Most macro substitutions one content line may perform; bounds recursive and fan-out growth.
const MAX_EXPANSIONS: usize = 1024;

/// Expands object-like macros, as Iris's preprocessor does in content and conditions. Numbers
/// and the spacing between tokens are left unchanged; in a condition, the operand of `defined`
/// stays a name. A macro inside its own expansion stays a name in content and reads as 0
/// in conditions. Exceeding `max_bytes` returns a recoverable diagnostic.
fn expand(
    text: &str,
    defines: &BTreeMap<String, String>,
    condition: bool,
    max_bytes: usize,
) -> Result<String, &'static str> {
    let mut budget = MAX_EXPANSIONS;
    let mut out = String::with_capacity(text.len().min(max_bytes));
    expand_into(
        text,
        defines,
        condition,
        &mut budget,
        &mut Vec::new(),
        &mut out,
        max_bytes,
    )?;
    Ok(out)
}

/// Appends `text` to `out`, replacing each defined identifier that is not already being
/// expanded (`active`) while `budget` lasts.
fn expand_into<'a>(
    text: &str,
    defines: &'a BTreeMap<String, String>,
    condition: bool,
    budget: &mut usize,
    active: &mut Vec<&'a str>,
    out: &mut String,
    max_bytes: usize,
) -> Result<(), &'static str> {
    let mut rest = text;
    let mut after_defined = false;
    while let Some((start, len)) = crate::options::next_code_word(rest) {
        append_expanded(out, &rest[..start], max_bytes)?;
        let word = &rest[start..start + len];
        rest = &rest[start + len..];
        match defines.get_key_value(word) {
            Some(_) if condition && active.contains(&word) && !after_defined => {
                append_expanded(out, "0", max_bytes)?;
            }
            Some((name, value))
                if *budget > 0
                    && !after_defined
                    && !word.as_bytes()[0].is_ascii_digit()
                    && !active.contains(&word) =>
            {
                *budget -= 1;
                active.push(name.as_str());
                expand_into(
                    flag_value(value),
                    defines,
                    condition,
                    budget,
                    active,
                    out,
                    max_bytes,
                )?;
                active.pop();
            }
            _ => append_expanded(out, word, max_bytes)?,
        }
        after_defined = condition && word == "defined";
    }
    append_expanded(out, rest, max_bytes)
}

/// Appends a fragment only if the expanded output stays within its byte budget.
fn append_expanded(out: &mut String, text: &str, max_bytes: usize) -> Result<(), &'static str> {
    if text.len() > max_bytes.saturating_sub(out.len()) {
        return Err("properties macro expansion exceeds byte limit");
    }
    out.push_str(text);
    Ok(())
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
/// `:` or property whitespace; whitespace and one `=`/`:` separator follow. Unescaping
/// preserves whitespace in keys and values, including escaped separators.
fn parse_entry(line: u32, logical: &str) -> RawEntry {
    let mut key_end = logical.len();
    let mut escaped = false;
    for (index, c) in logical.char_indices() {
        if escaped {
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == '=' || c == ':' || property_space(c) {
            key_end = index;
            break;
        }
    }
    let mut value = logical[key_end..].trim_start_matches(property_space);
    if let Some(stripped) = value.strip_prefix(['=', ':']) {
        value = stripped.trim_start_matches(property_space);
    }
    RawEntry {
        line,
        key: unescape(&logical[..key_end]),
        value: unescape(value),
    }
}

/// Whitespace that separates a Java property key from its value.
fn property_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\u{c}')
}

/// Java properties escapes: `\t \n \r \f`, `\uXXXX`, and `\c` → `c` for anything else. A
/// `\uXXXX` pair holding a UTF-16 surrogate pair decodes to its character, as Java's
/// UTF-16 strings hold it; a lone surrogate becomes U+FFFD.
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
                let unit = |at: usize| {
                    rest.get(at..at + 4)
                        .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                };
                if let Some(code) = unit(0) {
                    let low = rest
                        .get(4..6)
                        .filter(|&marker| marker == "\\u")
                        .and_then(|_| unit(6))
                        .filter(|low| (0xDC00..0xE000).contains(low));
                    let (decoded, used) = match (code, low) {
                        (0xD800..0xDC00, Some(low)) => (
                            char::from_u32(0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00)),
                            10,
                        ),
                        _ => (char::from_u32(code), 4),
                    };
                    out.push(decoded.unwrap_or(char::REPLACEMENT_CHARACTER));
                    chars = rest[used..].chars();
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

/// Removes directive comments while preserving quoted text and multi-line block state.
fn directive_code(mut text: &str, in_comment: &mut bool) -> String {
    let mut out = String::new();
    while !text.is_empty() {
        if *in_comment {
            let Some((_, rest)) = text.split_once("*/") else {
                break;
            };
            text = rest;
            *in_comment = false;
            out.push(' ');
        } else if text.starts_with("//") {
            break;
        } else if let Some(rest) = text.strip_prefix("/*") {
            text = rest;
            *in_comment = true;
            out.push(' ');
        } else if let Some(len) = crate::options::quoted_len(text) {
            out.push_str(&text[..len]);
            text = &text[len..];
        } else {
            let c = text.chars().next().unwrap();
            out.push(c);
            text = &text[c.len_utf8()..];
        }
    }
    out
}

/// A preprocessor directive with trimmed, comment-free arguments.
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
        let args = args.trim();
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
    /// Reports whether the current conditional branch accepts property entries.
    fn active(&self) -> bool {
        self.frames.last().is_none_or(|frame| frame.active)
    }

    /// Pushes a conditional frame while preserving the parent branch state.
    fn open(&mut self, line: u32, parent_active: bool, active: bool) {
        self.frames.push(Frame {
            line,
            active,
            taken: active || !parent_active,
            seen_else: false,
        });
    }

    /// Updates macros or conditional state and records malformed directives.
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

    /// Evaluates an `#if`/`#elif` condition with C preprocessor semantics (integer arithmetic,
    /// macro expansion, undefined names as 0, float literals as their integer part, as JCPP
    /// reads them); malformed conditions are reported and false.
    fn test(&self, text: &str, report: &mut impl FnMut(&str)) -> bool {
        // pp-rs mis-parses some operators after its own macro expansion, so names are expanded
        // here and pp-rs evaluates literals and `defined` only.
        let condition = match expand(text, &self.defines, true, MAX_EXPANDED_PROPERTIES_BYTES) {
            Ok(condition) => condition,
            Err(message) => {
                report(message);
                return false;
            }
        };
        let source = format!("#if {condition}\n1\n#else\n0\n#endif\n");
        let mut preprocessor = Preprocessor::new(&source);
        preprocessor.set_iris_conditions(true);
        for (name, value) in &self.defines {
            // A macro that cannot be defined is simply absent, as an undefined name.
            let _ = preprocessor.add_define(name, flag_value(value));
        }
        match preprocessor.next() {
            Some(Ok(token)) => {
                matches!(token.value, TokenValue::Integer(value) if value.value != 0)
            }
            Some(Err((error, _))) => {
                report(&format!("invalid condition `{text}`: {error:?}"));
                false
            }
            None => false,
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
    crate::options::split_ident(args).map(|(name, value)| (name, value.trim()))
}
