//! Shader pack options: discovery from shader sources and user overrides.
//!
//! OptiFine/Iris packs expose options through specially shaped lines in their GLSL
//! sources: boolean `#define` switches, `#define` values with an allowed-values list, and
//! `const` declarations. Effective values supply macros for property conditions and
//! program enable rules; shader source rewriting belongs to translation.

use std::collections::{BTreeMap, BTreeSet};

/// An option discovered in the pack's shader sources.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackOption {
    /// Macro or constant name.
    pub name: String,
    /// Shape and default of the option.
    pub kind: OptionKind,
    /// Exact pack-relative source key of the first definition. Directory keys use forward
    /// slashes; ZIP and memory keys retain their supplied spelling.
    pub file: String,
    /// 1-based line of the first definition in `file`.
    pub line: u32,
}

/// The kind of a [`PackOption`], with its default value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptionKind {
    /// `#define NAME` (default on) or `//#define NAME` (default off).
    Bool {
        /// Whether the define is active (not commented out) in the source.
        default: bool,
    },
    /// `#define NAME VALUE // [V1 V2 ...]` (the list may sit anywhere in the comment).
    Value {
        /// The value written in the source.
        default: String,
        /// The values listed in the trailing `[...]`.
        allowed: Vec<String>,
    },
    /// `const <ty> NAME = VALUE; // [V1 V2 ...]`, or `const bool NAME = true|false;`.
    /// `ty` is `int`, `float` or `bool`, as in OptiFine and Iris.
    Const {
        /// GLSL type of the constant.
        ty: String,
        /// The value written in the source.
        default: String,
        /// The values listed in the trailing `[...]`; empty for a `const bool` without a list.
        allowed: Vec<String>,
    },
}

/// User-chosen values; bools are "true"/"false". Unknown names and values outside
/// `allowed` are accepted as given (the pack may still compile).
pub type OptionValues = BTreeMap<String, String>;

/// Discovers options from every shader source file (`files` = (pack-relative path, text),
/// any order). Rules (OptiFine/Iris): the directive must start the line (leading whitespace
/// allowed); a boolean `#define`/`//#define` counts only if NAME is tested by `#ifdef NAME`,
/// `#ifndef NAME`, `defined NAME` or `defined(NAME)` somewhere in `files`; a value define needs
/// the `[...]` allowed-values list in its trailing `//` comment; `const` options must use a
/// configurable const name and need the list except `const bool`. First definition in sorted-path order wins; later conflicting
/// definitions are ignored. The result is sorted by name.
pub fn discover_options<'a>(
    files: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Vec<PackOption> {
    let mut files: Vec<_> = files
        .into_iter()
        .map(|(path, text)| (path, crate::source::normalize_newlines(text)))
        .collect();
    files.sort_by(|a, b| a.0.cmp(b.0));
    let tested = tested_names(files.iter().map(|(_, text)| text.as_ref()));
    let mut options: BTreeMap<&str, PackOption> = BTreeMap::new();
    for (path, text) in &files {
        for (index, line) in text.lines().enumerate() {
            let Some(definition) = parse_definition(line) else {
                continue;
            };
            if options.contains_key(definition.name) {
                continue;
            }
            let Some(kind) = option_kind(&definition, &tested) else {
                continue;
            };
            options.insert(
                definition.name,
                PackOption {
                    name: definition.name.to_owned(),
                    kind,
                    file: (*path).to_owned(),
                    line: u32::try_from(index + 1).unwrap_or(u32::MAX),
                },
            );
        }
    }
    options.into_values().collect()
}

impl PackOption {
    /// Effective value given overrides: "true"/"false" for Bool, else the value string.
    /// An override is returned as given, even when it is not an allowed value.
    pub fn value<'a>(&'a self, values: &'a OptionValues) -> &'a str {
        if let Some(value) = values.get(&self.name) {
            return value;
        }
        match &self.kind {
            OptionKind::Bool { default: true } => "true",
            OptionKind::Bool { default: false } => "false",
            OptionKind::Value { default, .. } | OptionKind::Const { default, .. } => default,
        }
    }
}

/// Macros the options define for the preprocessor and for `program.*.enabled` rules:
/// enabled Bool → (name, ""), Value → (name, value). Disabled Bools and Const options are absent.
/// A Bool is enabled when its effective value is exactly "true".
pub fn option_defines(options: &[PackOption], values: &OptionValues) -> BTreeMap<String, String> {
    let mut defines = BTreeMap::new();
    for option in options {
        let value = option.value(values);
        match option.kind {
            OptionKind::Bool { .. } => {
                if value == "true" {
                    defines.insert(option.name.clone(), String::new());
                }
            }
            OptionKind::Value { .. } => {
                defines.insert(option.name.clone(), value.to_owned());
            }
            OptionKind::Const { .. } => {}
        }
    }
    defines
}

/// Macros visible to `shaders.properties` conditions and enable rules: [`option_defines`]
/// plus const options, an enabled `const bool` as a flag and other consts by value.
pub(crate) fn property_defines(
    options: &[PackOption],
    values: &OptionValues,
) -> BTreeMap<String, String> {
    let mut defines = option_defines(options, values);
    for option in options {
        if let OptionKind::Const { ty, .. } = &option.kind {
            let value = option.value(values);
            if ty != "bool" {
                defines.insert(option.name.clone(), value.to_owned());
            } else if value == "true" {
                defines.insert(option.name.clone(), String::new());
            }
        }
    }
    defines
}

/// If `line` defines one of `options` and `values` overrides it, returns the rewritten line
/// keeping indentation and the trailing comment: Bool on → `#define NAME`, off → `//#define NAME`;
/// Value → `#define NAME <value>`; Const → `const <ty> NAME = <value>;`. Otherwise `None`.
/// Used during `#include` expansion, so every copy of the definition changes (Iris behaviour).
///
/// `options` must be sorted by name (as [`discover_options`] returns them); lookup is a
/// binary search. A line whose shape does not match the option's kind is left alone.
pub(crate) fn rewrite_option_line(
    line: &str,
    options: &[PackOption],
    values: &OptionValues,
) -> Option<String> {
    let definition = parse_definition(line)?;
    let index = options
        .binary_search_by(|option| option.name.as_str().cmp(definition.name))
        .ok()?;
    let option = options.get(index)?;
    let value = values.get(&option.name)?;
    let name = &option.name;
    let code = match (&option.kind, definition.form) {
        (OptionKind::Bool { .. }, Form::Switch { .. }) => {
            if value == "true" {
                format!("#define {name}")
            } else {
                format!("//#define {name}")
            }
        }
        (OptionKind::Value { .. }, Form::Value { .. }) => format!("#define {name} {value}"),
        (OptionKind::Const { ty, .. }, Form::Const { ty: line_ty, .. }) if ty == line_ty => {
            format!("const {ty} {name} = {value};")
        }
        _ => return None,
    };
    let indent = definition.indent;
    Some(match definition.comment {
        Some(comment) => format!("{indent}{code} {comment}"),
        None => format!("{indent}{code}"),
    })
}

/// Const names packs may expose as options; other `const` declarations are ordinary GLSL.
const CONFIGURABLE_CONSTS: &[&str] = &[
    "shadowMapResolution",
    "shadowDistance",
    "voxelDistance",
    "shadowDistanceRenderMul",
    "entityShadowDistanceMul",
    "shadowIntervalSize",
    "generateShadowMipmap",
    "generateShadowColorMipmap",
    "shadowHardwareFiltering",
    "shadowtex0Mipmap",
    "shadowtexMipmap",
    "shadowtex1Mipmap",
    "shadowtex0Nearest",
    "shadowtexNearest",
    "shadow0MinMagNearest",
    "shadowtex1Nearest",
    "shadow1MinMagNearest",
    "wetnessHalflife",
    "drynessHalflife",
    "eyeBrightnessHalflife",
    "centerDepthHalflife",
    "sunPathRotation",
    "ambientOcclusionLevel",
    "superSamplingLevel",
    "noiseTextureResolution",
];

/// Matches [`CONFIGURABLE_CONSTS`] and the per-buffer shadow-color consts for buffers 0–7.
fn is_configurable_const(name: &str) -> bool {
    let indexed = |prefix: &str, suffixes: &[&str]| {
        name.strip_prefix(prefix)
            .and_then(|rest| rest.split_at_checked(1))
            .is_some_and(|(index, suffix)| {
                matches!(index.as_bytes()[0], b'0'..=b'7') && suffixes.contains(&suffix)
            })
    };
    CONFIGURABLE_CONSTS.contains(&name)
        || ["shadowcolor", "shadowColor"]
            .into_iter()
            .any(|prefix| indexed(prefix, &["Mipmap", "Nearest", "MinMagNearest"]))
        || indexed("shadowHardwareFiltering", &[""])
}

/// A line shaped like an option definition (before the discovery rules are applied).
struct Definition<'a> {
    /// Leading whitespace of the line.
    indent: &'a str,
    name: &'a str,
    form: Form<'a>,
    /// Trailing comments, starting at `//` or `/*`.
    comment: Option<&'a str>,
}

enum Form<'a> {
    /// `#define NAME` (enabled) or `//#define NAME` (disabled).
    Switch { enabled: bool },
    /// `#define NAME VALUE` with a single-token value.
    Value { value: &'a str },
    /// `const <int|float|bool> NAME = VALUE;` with a single-token value.
    Const { ty: &'a str, value: &'a str },
}

/// Infers an option kind from its declaration, allowed values and conditional uses.
fn option_kind(definition: &Definition<'_>, tested: &BTreeSet<&str>) -> Option<OptionKind> {
    let allowed = allowed_values(definition.comment);
    match definition.form {
        Form::Switch { enabled } => {
            if tested.contains(definition.name) {
                Some(OptionKind::Bool { default: enabled })
            } else {
                None
            }
        }
        Form::Value { value } => Some(OptionKind::Value {
            default: value.to_owned(),
            allowed: allowed?,
        }),
        Form::Const { ty, value } => {
            if !is_configurable_const(definition.name) {
                return None;
            }
            let allowed = if ty == "bool" {
                if value != "true" && value != "false" {
                    return None;
                }
                allowed.unwrap_or_default()
            } else {
                allowed?
            };
            Some(OptionKind::Const {
                ty: ty.to_owned(),
                default: value.to_owned(),
                allowed,
            })
        }
    }
}

/// The values of the first `[V1 V2 ...]` list anywhere in a `//` comment; `None` without a
/// non-empty list.
fn allowed_values(comment: Option<&str>) -> Option<Vec<String>> {
    let (_, list) = comment?.strip_prefix("//")?.split_once('[')?;
    let (list, _) = list.split_once(']')?;
    let values: Vec<String> = list.split_whitespace().map(str::to_owned).collect();
    if values.is_empty() {
        None
    } else {
        Some(values)
    }
}

/// Reads a single-line option declaration and its trailing comment.
fn parse_definition(line: &str) -> Option<Definition<'_>> {
    let body = line.trim_start();
    let indent = &line[..line.len() - body.len()];
    if let Some(rest) = body.strip_prefix("const") {
        return parse_const(indent, rest);
    }
    let (enabled, rest) = match body.strip_prefix("//") {
        Some(rest) => (false, rest.trim_start()),
        None => (true, body),
    };
    let rest = rest.strip_prefix("#define")?;
    let (name, rest) = split_ident(skip_required_space(rest)?)?;
    // Function-like macros have no whitespace or comment between the name and `(`.
    if !rest.is_empty()
        && !rest.starts_with(char::is_whitespace)
        && !rest.starts_with("//")
        && !rest.starts_with("/*")
    {
        return None;
    }
    if rest.trim_start().starts_with("/*") {
        return only_block_comments(rest).then_some(Definition {
            indent,
            name,
            form: Form::Switch { enabled },
            comment: Some(rest.trim()),
        });
    }
    let (code, comment) = split_comment(rest);
    let value = code.trim();
    let form = if value.is_empty() {
        Form::Switch { enabled }
    } else if enabled && !value.contains(char::is_whitespace) {
        Form::Value { value }
    } else {
        return None;
    };
    Some(Definition {
        indent,
        name,
        form,
        comment,
    })
}

/// `rest` follows a line-leading `const`; `indent` is the whitespace before it.
fn parse_const<'a>(indent: &'a str, rest: &'a str) -> Option<Definition<'a>> {
    let (ty, rest) = split_ident(skip_required_space(rest)?)?;
    if !matches!(ty, "int" | "float" | "bool") {
        return None;
    }
    let (name, rest) = split_ident(skip_required_space(rest)?)?;
    let rest = rest.trim_start().strip_prefix('=')?;
    let (value, rest) = rest.split_once(';')?;
    let value = value.trim();
    if value.is_empty() || value.contains(char::is_whitespace) {
        return None;
    }
    let rest = rest.trim_start();
    let comment = if rest.is_empty() {
        None
    } else if rest.starts_with("//") {
        Some(rest)
    } else {
        return None;
    };
    Some(Definition {
        indent,
        name,
        form: Form::Const { ty, value },
        comment,
    })
}

/// `text` after required leading whitespace, or `None` when no whitespace starts it.
fn skip_required_space(text: &str) -> Option<&str> {
    if text.starts_with(char::is_whitespace) {
        Some(text.trim_start())
    } else {
        None
    }
}

/// Splits `code // comment` at the first `//`; the comment keeps its slashes and is trimmed.
fn split_comment(text: &str) -> (&str, Option<&str>) {
    match text.find("//") {
        Some(index) => (&text[..index], Some(text[index..].trim_end())),
        None => (text, None),
    }
}

/// Checks whether a byte can appear within a C-style identifier.
fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Splits a leading C identifier off `text`.
pub(crate) fn split_ident(text: &str) -> Option<(&str, &str)> {
    let len = text.bytes().take_while(|&b| is_ident_byte(b)).count();
    let (ident, rest) = text.split_at(len);
    if ident.bytes().next().is_some_and(|b| !b.is_ascii_digit()) {
        Some((ident, rest))
    } else {
        None
    }
}

/// Reports whether only closed block comments, whitespace and an optional line comment remain.
fn only_block_comments(mut text: &str) -> bool {
    text = text.trim();
    while let Some(comment) = text.strip_prefix("/*") {
        let Some((_, rest)) = comment.split_once("*/") else {
            return false;
        };
        text = rest.trim_start();
    }
    text.is_empty() || text.starts_with("//")
}

/// Returns the byte length of a leading quoted token, including its escapes.
pub(crate) fn quoted_len(text: &str) -> Option<usize> {
    let quote = text.chars().next()?;
    if !matches!(quote, '\'' | '"') {
        return None;
    }
    let mut escaped = false;
    for (index, c) in text.char_indices().skip(1) {
        if escaped {
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == quote {
            return Some(index + c.len_utf8());
        }
    }
    Some(text.len())
}

/// Finds the next preprocessing word outside quoted tokens and comments.
pub(crate) fn next_code_word(text: &str) -> Option<(usize, usize)> {
    let mut offset = 0;
    while offset < text.len() {
        let rest = &text[offset..];
        if rest.starts_with("//") {
            return None;
        }
        if let Some(comment) = rest.strip_prefix("/*") {
            offset += comment.find("*/").map_or(rest.len(), |end| end + 4);
            continue;
        }
        if let Some(len) = quoted_len(rest) {
            offset += len;
            continue;
        }
        let len = rest.bytes().take_while(|&b| is_ident_byte(b)).count();
        if len != 0 {
            return Some((offset, len));
        }
        offset += rest.chars().next()?.len_utf8();
    }
    None
}

/// Names tested by `#ifdef`, `#ifndef` or `defined` in `#if`/`#elif` lines of any file.
fn tested_names<'a>(files: impl IntoIterator<Item = &'a str>) -> BTreeSet<&'a str> {
    let mut names = BTreeSet::new();
    for text in files {
        for line in text.lines() {
            let Some(directive) = line.trim_start().strip_prefix('#') else {
                continue;
            };
            let Some((keyword, rest)) = split_ident(directive.trim_start()) else {
                continue;
            };
            match keyword {
                "ifdef" | "ifndef" => {
                    if let Some((start, len)) = next_code_word(rest)
                        && let Some((name, _)) = split_ident(&rest[start..start + len])
                    {
                        names.insert(name);
                    }
                }
                "if" | "elif" => collect_defined(rest, &mut names),
                _ => {}
            }
        }
    }
    names
}

/// Adds the operands of every uncommented `defined NAME` / `defined(NAME)` in `text`.
fn collect_defined<'a>(mut text: &'a str, names: &mut BTreeSet<&'a str>) {
    while let Some((start, len)) = next_code_word(text) {
        let word = &text[start..start + len];
        text = &text[start + len..];
        if word != "defined" {
            continue;
        }
        let Some((start, len)) = next_code_word(text) else {
            break;
        };
        if let Some((name, _)) = split_ident(&text[start..start + len]) {
            names.insert(name);
        }
        text = &text[start + len..];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Supplies one shared path per original test file to option discovery.
    fn discover<'a>(files: impl IntoIterator<Item = (&'a str, &'a str)>) -> Vec<PackOption> {
        discover_options(files)
    }

    /// Builds owned option overrides from borrowed test values.
    fn values(pairs: &[(&str, &str)]) -> OptionValues {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    }

    /// Copies borrowed strings into an owned list for assertions.
    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    /// Finds the discovered option with the requested name.
    fn find<'a>(options: &'a [PackOption], name: &str) -> Option<&'a PackOption> {
        options.iter().find(|option| option.name == name)
    }

    const SETTINGS: &str = "\
#define SOFT_SHADOWS
#define UNTESTED_SWITCH
//#define GLOW
// #define SPACED_OFF // a description
    #define INDENTED_ON
#define QUALITY 2 // [1 2 3] Rendering quality
#define TIGHT 0.5 //[0.25 0.5 1.0]
#define LEVEL 2 // Quality level [1 2 3] for shadows
#define NO_LIST 4
#define EMPTY_LIST 4 // []
//#define COMMENTED_VALUE 3 // [1 2 3]
#define MACRO_FN(x) x // [1 2]
int late; #define LATE 1 // [1 2]
const int shadowMapResolution = 2048; // [1024 2048 4096]
const int samples = 8; // [4 8 16]
const float sunPathRotation = 1.0;
const bool generateShadowMipmap = true;
const bool shadowtex0Nearest = sometimes;
const vec3 tint = vec3(1.0); // [a b]
";

    const USES: &str = "\
#ifdef SOFT_SHADOWS
#endif
#ifndef INDENTED_ON
#endif
#if defined(GLOW) && !defined SPACED_OFF || definedLATE
#elif defined ( LATE_TEST )
#endif
// #ifdef UNTESTED_SWITCH appears only in a comment
";

    #[test]
    fn frontend_option_discovery_uses_all_glsl_newlines() {
        for newline in ["\r", "\r\n", "\n"] {
            let text = [
                "#version 450",
                "#define FEATURE /* note */",
                "#ifdef FEATURE",
                "#endif",
            ]
            .join(newline);
            let options = discover_options([("a.fsh", text.as_str())]);
            assert_eq!(options.len(), 1);
            assert_eq!(options[0].line, 2);
        }
    }

    #[test]
    fn discovery_rules_for_every_kind() {
        let options = discover([
            ("shaders/settings.glsl", SETTINGS),
            ("shaders/uses.glsl", USES),
        ]);
        let names: Vec<&str> = options.iter().map(|option| option.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "GLOW",
                "INDENTED_ON",
                "LEVEL",
                "QUALITY",
                "SOFT_SHADOWS",
                "SPACED_OFF",
                "TIGHT",
                "generateShadowMipmap",
                "shadowMapResolution",
            ]
        );
        let kind = |name: &str| find(&options, name).map(|option| option.kind.clone());
        assert_eq!(
            kind("SOFT_SHADOWS"),
            Some(OptionKind::Bool { default: true })
        );
        assert_eq!(
            kind("INDENTED_ON"),
            Some(OptionKind::Bool { default: true })
        );
        assert_eq!(kind("GLOW"), Some(OptionKind::Bool { default: false }));
        assert_eq!(
            kind("SPACED_OFF"),
            Some(OptionKind::Bool { default: false })
        );
        assert_eq!(
            kind("QUALITY"),
            Some(OptionKind::Value {
                default: "2".to_owned(),
                allowed: strings(&["1", "2", "3"]),
            })
        );
        assert_eq!(
            kind("LEVEL"),
            Some(OptionKind::Value {
                default: "2".to_owned(),
                allowed: strings(&["1", "2", "3"]),
            })
        );
        assert_eq!(
            kind("TIGHT"),
            Some(OptionKind::Value {
                default: "0.5".to_owned(),
                allowed: strings(&["0.25", "0.5", "1.0"]),
            })
        );
        assert_eq!(
            kind("shadowMapResolution"),
            Some(OptionKind::Const {
                ty: "int".to_owned(),
                default: "2048".to_owned(),
                allowed: strings(&["1024", "2048", "4096"]),
            })
        );
        assert_eq!(
            kind("generateShadowMipmap"),
            Some(OptionKind::Const {
                ty: "bool".to_owned(),
                default: "true".to_owned(),
                allowed: Vec::new(),
            })
        );
        let quality = find(&options, "QUALITY").expect("QUALITY");
        assert_eq!(
            (quality.file.as_str(), quality.line),
            ("shaders/settings.glsl", 6)
        );
    }

    #[test]
    fn first_definition_in_sorted_path_order_wins() {
        let later = "#define LEVEL 1 // [1 2]\n#ifdef TOGGLE\n#endif\n";
        let earlier = "\n#define LEVEL 2 // [2 3]\n#define TOGGLE\n";
        let options = discover([
            ("shaders/zeta.glsl", later),
            ("shaders/alpha.glsl", earlier),
        ]);
        let level = find(&options, "LEVEL").expect("LEVEL");
        assert_eq!(
            level.kind,
            OptionKind::Value {
                default: "2".to_owned(),
                allowed: strings(&["2", "3"]),
            }
        );
        assert_eq!((level.file.as_str(), level.line), ("shaders/alpha.glsl", 2));
        let toggle = find(&options, "TOGGLE").expect("TOGGLE");
        assert_eq!(toggle.kind, OptionKind::Bool { default: true });
    }

    #[test]
    fn conflicting_later_definitions_are_ignored() {
        let text = "#ifdef MODE\n#endif\n//#define MODE\n#define MODE\n#define MODE 3 // [3 4]\n";
        let options = discover([("a.glsl", text)]);
        assert_eq!(options.len(), 1);
        assert_eq!(options[0].kind, OptionKind::Bool { default: false });
        assert_eq!(options[0].line, 3);
    }

    #[test]
    fn effective_values_and_defines() {
        let options = discover([
            ("shaders/settings.glsl", SETTINGS),
            ("shaders/uses.glsl", USES),
        ]);
        let overrides = values(&[
            ("GLOW", "true"),
            ("SOFT_SHADOWS", "false"),
            ("QUALITY", "5"),
        ]);
        let value = |name: &str| find(&options, name).map(|option| option.value(&overrides));
        assert_eq!(value("GLOW"), Some("true"));
        assert_eq!(value("SOFT_SHADOWS"), Some("false"));
        assert_eq!(value("SPACED_OFF"), Some("false"));
        assert_eq!(value("QUALITY"), Some("5"));
        assert_eq!(value("TIGHT"), Some("0.5"));
        assert_eq!(value("shadowMapResolution"), Some("2048"));

        let defines = option_defines(&options, &overrides);
        let expected: BTreeMap<String, String> = [
            ("GLOW", ""),
            ("INDENTED_ON", ""),
            ("LEVEL", "2"),
            ("QUALITY", "5"),
            ("TIGHT", "0.5"),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .collect();
        assert_eq!(defines, expected);
    }

    #[test]
    fn rewrite_option_line_for_every_kind() {
        let options = discover([
            ("shaders/settings.glsl", SETTINGS),
            ("shaders/uses.glsl", USES),
        ]);
        let overrides = values(&[
            ("SOFT_SHADOWS", "false"),
            ("GLOW", "true"),
            ("INDENTED_ON", "false"),
            ("QUALITY", "3"),
            ("shadowMapResolution", "4096"),
            ("generateShadowMipmap", "false"),
        ]);
        let rewrite = |line: &str| rewrite_option_line(line, &options, &overrides);
        assert_eq!(
            rewrite("#define SOFT_SHADOWS"),
            Some("//#define SOFT_SHADOWS".to_owned())
        );
        assert_eq!(rewrite("//#define GLOW"), Some("#define GLOW".to_owned()));
        assert_eq!(
            rewrite("    #define INDENTED_ON // keep me"),
            Some("    //#define INDENTED_ON // keep me".to_owned())
        );
        assert_eq!(
            rewrite("\t#define QUALITY 2 // [1 2 3] Rendering quality"),
            Some("\t#define QUALITY 3 // [1 2 3] Rendering quality".to_owned())
        );
        assert_eq!(
            rewrite("const int shadowMapResolution = 2048; // [1024 2048 4096]"),
            Some("const int shadowMapResolution = 4096; // [1024 2048 4096]".to_owned())
        );
        assert_eq!(
            rewrite("  const bool generateShadowMipmap = true;"),
            Some("  const bool generateShadowMipmap = false;".to_owned())
        );
    }

    #[test]
    fn rewrite_leaves_other_lines_alone() {
        let options = discover([
            ("shaders/settings.glsl", SETTINGS),
            ("shaders/uses.glsl", USES),
        ]);
        let overrides = values(&[
            ("QUALITY", "3"),
            ("UNTESTED_SWITCH", "false"),
            ("shadowMapResolution", "1024"),
        ]);
        let rewrite = |line: &str| rewrite_option_line(line, &options, &overrides);
        // Not overridden.
        assert_eq!(rewrite("#define TIGHT 0.5 //[0.25 0.5 1.0]"), None);
        // Not an option.
        assert_eq!(rewrite("#define UNTESTED_SWITCH"), None);
        // Shape does not match the option's kind.
        assert_eq!(rewrite("#define QUALITY"), None);
        assert_eq!(
            rewrite("const float shadowMapResolution = 2048.0; // [1024 2048]"),
            None
        );
        // Not a definition at all.
        assert_eq!(rewrite("float q = QUALITY * 2.0;"), None);
        assert_eq!(rewrite("x = 1; #define QUALITY 2 // [1 2 3]"), None);
    }

    #[test]
    fn boolean_options_accept_adjacent_comments() {
        for declaration in [
            "#define BLOOM// description",
            "#define BLOOM/* description */",
            "//#define BLOOM// description",
        ] {
            let source = format!("{declaration}\n#ifdef BLOOM\n#endif\n");
            let options = discover([("a.glsl", source.as_str())]);
            let bloom = find(&options, "BLOOM").expect("boolean option");
            assert_eq!(
                bloom.kind,
                OptionKind::Bool {
                    default: !declaration.starts_with("//")
                }
            );
        }
        let options = discover([("a.glsl", "#define BLOOM(x) x\n#ifdef BLOOM\n#endif\n")]);
        assert!(options.is_empty());
    }

    #[test]
    fn comments_do_not_count_as_boolean_option_references() {
        let options = discover([(
            "a.glsl",
            "#define BLOOM\n#define GHOST\n#if defined BLOOM // defined GHOST\n#endif\n#ifnot_a_directive defined GHOST\n",
        )]);
        assert!(find(&options, "BLOOM").is_some());
        assert!(find(&options, "GHOST").is_none());
    }

    #[test]
    fn whitespace_and_reference_forms_discover_boolean_options() {
        for whitespace in [" ", "\t", "\u{c}", "\u{b}"] {
            for reference in ["#ifndef BLOOM", "#if defined BLOOM", "#elif defined(BLOOM)"] {
                let source = format!("#define{whitespace}BLOOM\r\n{reference}\r\n");
                let options = discover([("a.glsl", source.as_str())]);
                assert_eq!(
                    find(&options, "BLOOM").map(|option| &option.kind),
                    Some(&OptionKind::Bool { default: true })
                );
            }
        }
    }
}
