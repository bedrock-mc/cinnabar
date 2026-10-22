//! The shared rule file, compiled once, and the single pass Go's `redact.Text` mirrors.

use std::{ops::Range, sync::LazyLock};

use regex::Regex;
use serde::Deserialize;

/// The rule file. The Go core embeds a byte-for-byte copy as `core/internal/redact/rules.json`.
pub(super) const RULES_JSON: &str = include_str!("../../data/redaction-rules.json");

#[derive(Deserialize)]
struct File {
    placeholder: String,
    fields: Fields,
    source_location: String,
    timestamp: String,
    file_extensions: Vec<String>,
    rules: Vec<Spec>,
}

/// Field-name alternatives whose values a rule removes; JSON keys use both lists.
#[derive(Deserialize)]
struct Fields {
    credential: Vec<String>,
    private: Vec<String>,
}

#[derive(Deserialize)]
struct Spec {
    pattern: String,
    /// The `fields` list substituted for `{fields}` in `pattern`.
    #[serde(default)]
    fields: Option<String>,
    #[serde(default)]
    check: Option<Check>,
}

/// A match-level test for rules a regular expression alone cannot express.
#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Check {
    /// Only standalone, parseable IPv6 addresses; Rust paths and `h:m:s` times survive.
    Ipv6,
    /// Neither a source location, a timestamp nor a bare filename with a listed extension.
    Host,
}

/// One compiled rule. With capture groups, every participating group is replaced and the rest
/// of the match kept; without, the whole match (as narrowed by `check`) is replaced.
pub(super) struct Rule {
    pub(super) pattern: Regex,
    check: Option<Check>,
}

pub(super) struct Rules {
    pub(super) placeholder: String,
    /// Matches a whole field name from either list, for JSON keys.
    pub(super) field_key: Regex,
    pub(super) rules: Vec<Rule>,
    source_location: Regex,
    timestamp: Regex,
    file_extensions: Vec<String>,
}

/// The compiled rule file. A test proves the embedded file parses and compiles.
pub(super) static RULES: LazyLock<Rules> =
    LazyLock::new(|| compile(RULES_JSON).expect("the embedded redaction rules compile"));

/// Parses and compiles a rule file, naming the first rule that fails.
pub(super) fn compile(json: &str) -> Result<Rules, String> {
    let file: File = serde_json::from_str(json).map_err(|error| error.to_string())?;
    let list = |name: &str| match name {
        "credential" => Ok(file.fields.credential.join("|")),
        "private" => Ok(file.fields.private.join("|")),
        other => Err(format!("unknown field list `{other}`")),
    };
    let all = format!("{}|{}", list("credential")?, list("private")?);
    let rules = file
        .rules
        .iter()
        .map(|spec| {
            let pattern = match &spec.fields {
                Some(name) => spec.pattern.replace("{fields}", &list(name)?),
                None => spec.pattern.clone(),
            };
            Ok(Rule {
                pattern: regex(&pattern)?,
                check: spec.check,
            })
        })
        .collect::<Result<_, String>>()?;
    Ok(Rules {
        placeholder: file.placeholder,
        field_key: regex(&format!("(?i)^(?:{all})$"))?,
        rules,
        source_location: regex(&file.source_location)?,
        timestamp: regex(&file.timestamp)?,
        file_extensions: file.file_extensions,
    })
}

/// Compiles a rule-file pattern with Go's ASCII word boundary, so `\b` agrees in both languages.
/// The file spells other classes in ASCII (`[0-9]`, `[[:space:]]`) for the same reason.
fn regex(pattern: &str) -> Result<Regex, String> {
    let mut ascii = String::with_capacity(pattern.len());
    let mut chars = pattern.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            ascii.push(c);
            continue;
        }
        match chars.next() {
            Some('b') => ascii.push_str(r"(?-u:\b)"),
            Some(next) => {
                ascii.push('\\');
                ascii.push(next);
            }
            None => ascii.push('\\'),
        }
    }
    Regex::new(&ascii).map_err(|error| format!("{pattern}: {error}"))
}

impl Rules {
    /// Applies every rule once, in file order.
    pub(super) fn apply(&self, text: &str, file_name: bool) -> String {
        let mut text = text.to_owned();
        for rule in &self.rules {
            if let Some(next) = self.apply_rule(rule, &text, file_name) {
                text = next;
            }
        }
        text
    }

    /// The text with `rule`'s spans replaced, or `None` when nothing changed.
    fn apply_rule(&self, rule: &Rule, text: &str, file_name: bool) -> Option<String> {
        let sources = if rule.check == Some(Check::Host) {
            self.source_locations(text)
        } else {
            Vec::new()
        };
        let mut out = String::new();
        let mut last = 0;
        let mut replace = |span: Range<usize>| {
            out.push_str(&text[last..span.start]);
            out.push_str(&self.placeholder);
            last = span.end;
        };
        for captures in rule.pattern.captures_iter(text) {
            if captures.len() > 1 {
                captures
                    .iter()
                    .skip(1)
                    .flatten()
                    .for_each(|group| replace(group.range()));
                continue;
            }
            let whole = captures.get(0).expect("a match has group 0").range();
            let span = match rule.check {
                None => Some(whole),
                Some(Check::Ipv6) => ipv6_span(text, whole),
                Some(Check::Host) if file_name && !text.contains([':', '/', '\\']) => None,
                Some(Check::Host) => {
                    (!self.keeps_host(&text[whole.clone()], &whole, &sources)).then_some(whole)
                }
            };
            if let Some(span) = span {
                replace(span);
            }
        }
        if last == 0 && out.is_empty() {
            return None;
        }
        out.push_str(&text[last..]);
        Some(out)
    }

    /// Source locations with a path separator or column, ISO timestamps and line-leading log times.
    /// Both may contain colons and digits without naming a server.
    fn source_locations(&self, text: &str) -> Vec<Range<usize>> {
        self.source_location
            .captures_iter(text)
            .filter_map(|captures| {
                let whole = captures.get(0)?;
                (captures.get(1).is_some() || text[..whole.start()].ends_with(['/', '\\']))
                    .then(|| whole.range())
            })
            .chain(
                self.timestamp
                    .find_iter(text)
                    .map(|matched| matched.range()),
            )
            .collect()
    }

    /// Whether a host match is a source location or a file name rather than a server.
    fn keeps_host(&self, host: &str, span: &Range<usize>, sources: &[Range<usize>]) -> bool {
        if sources
            .iter()
            .any(|source| source.start <= span.start && span.end <= source.end)
        {
            return true;
        }
        if host.rsplit_once(':').is_some_and(|(_, port)| {
            !port.is_empty() && port.bytes().all(|byte| byte.is_ascii_digit())
        }) {
            return false;
        }
        host.rsplit_once('.').is_some_and(|(_, extension)| {
            self.file_extensions
                .iter()
                .any(|listed| listed.eq_ignore_ascii_case(extension))
        })
    }
}

/// The address within an IPv6 candidate match, when it is a standalone address: a trailing dot
/// or prose colon is not part of it, and a Rust path such as `<u32 as Add>::add` is not one.
fn ipv6_span(text: &str, whole: Range<usize>) -> Option<Range<usize>> {
    let identifier = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let path = |c: char| identifier(c) || matches!(c, '>' | '}' | ')' | ':');
    let mut address = text[whole.clone()].trim_end_matches('.');
    if !ipv6_address(address) && address.ends_with(':') && !address.ends_with("::") {
        address = &address[..address.len() - 1];
    }
    let end = whole.start + address.len();
    let standalone = !text[..whole.start].ends_with(path) && !text[end..].starts_with(identifier);
    (standalone && ipv6_address(address)).then_some(whole.start..end)
}

/// Whether a candidate, with optional brackets, zone and port, is an address with a hex group.
fn ipv6_address(candidate: &str) -> bool {
    let mut addresses = Vec::with_capacity(2);
    match candidate.strip_prefix('[') {
        Some(rest) => addresses.push(rest.split_once(']').map_or(rest, |(address, _)| address)),
        None => {
            addresses.push(candidate);
            if let Some((address, port)) = candidate.rsplit_once(':')
                && !port.is_empty()
                && port.bytes().all(|byte| byte.is_ascii_digit())
            {
                addresses.push(address);
            }
        }
    }
    addresses.into_iter().any(|address| {
        let address = address
            .split_once('%')
            .map_or(address, |(address, _)| address);
        address.parse::<std::net::Ipv6Addr>().is_ok()
            && address.split(':').any(|group| !group.is_empty())
    })
}
