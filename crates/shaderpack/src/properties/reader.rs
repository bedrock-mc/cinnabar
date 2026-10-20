//! Line-level reading of `shaders.properties` in Iris's two stages: a line-by-line
//! conditional preprocessor, then java.util.Properties over the surviving text. Directives
//! are recognised on every physical line, so a continuation never joins one.

use std::collections::BTreeMap;

use super::PropertiesDiagnostic;
use crate::expr::truncated;
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
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    for (index, physical) in text.lines().enumerate() {
        let line = u32::try_from(index + 1).unwrap_or(u32::MAX);
        let trimmed = physical.trim();
        if trimmed.is_empty() {
            continue;
        }
        let kept = if let Some(rest) = trimmed.strip_prefix('#') {
            if let Some(directive) = Directive::parse(rest) {
                conditionals.apply(directive, line, diagnostics);
            }
            String::new()
        } else if conditionals.active() {
            // Iris strips `#` from content so the preprocessor never reads it as a directive.
            expand(&trimmed.replace('#', ""), &conditionals.defines, false)
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
        let text = physical.trim_start();
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
/// stays a name. As in C, a macro met inside its own expansion is not expanded again: it stays
/// a name in content and counts as 0 in a condition.
fn expand(text: &str, defines: &BTreeMap<String, String>, condition: bool) -> String {
    let mut budget = MAX_EXPANSIONS;
    let mut out = String::with_capacity(text.len());
    expand_into(
        text,
        defines,
        condition,
        &mut budget,
        &mut Vec::new(),
        &mut out,
    );
    out
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
) {
    let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut rest = text;
    let mut after_defined = false;
    while let Some(start) = rest.bytes().position(is_word) {
        out.push_str(&rest[..start]);
        let len = rest[start..].bytes().take_while(|&b| is_word(b)).count();
        let word = &rest[start..start + len];
        rest = &rest[start + len..];
        match defines.get_key_value(word) {
            Some(_) if condition && active.contains(&word) && !after_defined => out.push('0'),
            Some((name, value))
                if *budget > 0
                    && !after_defined
                    && !word.as_bytes()[0].is_ascii_digit()
                    && !active.contains(&word) =>
            {
                *budget -= 1;
                active.push(name.as_str());
                expand_into(flag_value(value), defines, condition, budget, active, out);
                active.pop();
            }
            _ => out.push_str(word),
        }
        after_defined = condition && word == "defined";
    }
    out.push_str(rest);
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
/// unescaped and trimmed, except that a value keeps an escaped leading space (`\ `) as
/// java.util.Properties does.
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
        value: unescape(value).trim_end().to_owned(),
    }
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
        // here and pp-rs evaluates literals and `defined` only. pp-rs rejects float literals.
        let condition = integer_literals(&expand(text, &self.defines, true));
        let source = format!("#if {condition}\n1\n#else\n0\n#endif\n");
        let mut preprocessor = Preprocessor::new(&source);
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

/// `condition` with each float literal replaced by its integer part (`0.75 > 0.5` → `0 > 0`).
/// Identifiers are copied whole, so digits inside names are untouched.
fn integer_literals(condition: &str) -> String {
    let mut out = String::with_capacity(condition.len());
    let mut rest = condition;
    while let Some(c) = rest.chars().next() {
        let starts_number =
            c.is_ascii_digit() || (c == '.' && rest[1..].starts_with(|d: char| d.is_ascii_digit()));
        if !starts_number && !(c.is_ascii_alphabetic() || c == '_') {
            out.push(c);
            rest = &rest[c.len_utf8()..];
            continue;
        }
        // An identifier, or a C preprocessing number: digits, letters, `.`, and a sign after an
        // exponent.
        let mut previous = ' ';
        let end = rest
            .find(|c: char| {
                let exponent_sign = matches!(c, '+' | '-') && matches!(previous, 'e' | 'E');
                let number_part = starts_number && (c == '.' || exponent_sign);
                previous = c;
                !(c.is_ascii_alphanumeric() || c == '_' || number_part)
            })
            .unwrap_or(rest.len());
        let (token, tail) = rest.split_at(end);
        rest = tail;
        let integer = if starts_number {
            truncated(token)
        } else {
            None
        };
        out.push_str(integer.as_deref().unwrap_or(token));
    }
    out
}
