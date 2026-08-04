use std::{cell::Cell, sync::Arc};

use serde::{
    Deserialize, Deserializer,
    de::{DeserializeSeed, IgnoredAny, MapAccess, Visitor},
};

use crate::ui::UiPacketError;

pub const MAX_RAW_TEXT_INPUT_BYTES: usize = crate::ui::MAX_UI_TEXT_BYTES;
pub const MAX_RAW_TEXT_NODES: usize = 768;
pub const MAX_RAW_TEXT_DEPTH: usize = 16;
pub const MAX_RAW_TEXT_COMPONENTS: usize = 256;
pub const MAX_RAW_TEXT_OUTPUT_BYTES: usize = 8_192;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawTextDocument {
    components: Arc<[RawTextComponent]>,
    literal_text: Arc<str>,
    resolution: RawTextResolution,
}

impl RawTextDocument {
    #[must_use]
    pub fn components(&self) -> &[RawTextComponent] {
        &self.components
    }

    #[must_use]
    pub fn literal_text(&self) -> &str {
        &self.literal_text
    }

    #[must_use]
    pub const fn resolution(&self) -> RawTextResolution {
        self.resolution
    }

    #[must_use]
    pub const fn has_unresolved_components(&self) -> bool {
        matches!(self.resolution, RawTextResolution::RequiresResolver)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RawTextResolution {
    /// Every retained component is literal text and can be displayed without external state.
    LiteralOnly,
    /// Translation, scoreboard, or selector state must be resolved by an authoritative catalog.
    RequiresResolver,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RawTextComponent {
    Text(Arc<str>),
    Translate {
        key: Arc<str>,
        with: Arc<[RawTextComponent]>,
    },
    Score {
        name: Arc<str>,
        objective: Arc<str>,
    },
    Selector(Arc<str>),
    Sequence(Arc<[RawTextComponent]>),
}

/// External authoritative state for one rawtext resolution pass.
///
/// The document itself never learns to query stores; the caller lends bounded
/// lookups. `translate` returns the localized template for a key, or `None`
/// for an unknown key — the vanilla client then presents the raw key.
/// `score` resolves an owner display name and objective to a value; the
/// reader sentinel `*` is replaced with `reader_name` before lookup.
/// `selector` resolves the selector patterns the retained authoritative
/// state can answer (`@s`, the known player list for `@a`); returning `None`
/// counts the selector as skipped and presents it as empty text.
pub struct RawTextResolver<'a> {
    pub reader_name: &'a str,
    pub translate: &'a dyn Fn(&str) -> Option<Arc<str>>,
    pub score: &'a dyn Fn(&str, &str) -> Option<i32>,
    pub selector: &'a dyn Fn(&str) -> Option<Arc<str>>,
}

/// The outcome of resolving one document: human text plus counters for every
/// component the lent state could not answer. Nothing in `text` is JSON.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ResolvedRawText {
    pub text: String,
    pub unknown_translations: u32,
    pub unresolved_scores: u32,
    /// Selectors the lent state could not answer. The vanilla server
    /// resolves selectors before sending; the reader (`@s`) and the known
    /// player list (`@a`) resolve from retained state, and anything needing
    /// a live entity query presents as empty text and counts here.
    pub skipped_selectors: u32,
    /// Set when the bounded output budget truncated the resolved text.
    pub truncated: bool,
}

impl RawTextDocument {
    /// Resolves every component into presentable text, bounded by
    /// [`MAX_RAW_TEXT_OUTPUT_BYTES`]. Components the resolver cannot answer
    /// degrade exactly like the vanilla client: unknown translation keys
    /// present the key itself, unresolvable scores and selectors present as
    /// empty text, and each degradation is counted for diagnostics.
    #[must_use]
    pub fn resolve(&self, resolver: &RawTextResolver<'_>) -> ResolvedRawText {
        let mut resolved = ResolvedRawText::default();
        for component in self.components.iter() {
            resolve_component(component, resolver, &mut resolved, 0);
        }
        resolved
    }
}

fn resolve_component(
    component: &RawTextComponent,
    resolver: &RawTextResolver<'_>,
    resolved: &mut ResolvedRawText,
    depth: usize,
) {
    if depth > MAX_RAW_TEXT_DEPTH {
        resolved.truncated = true;
        return;
    }
    match component {
        RawTextComponent::Text(text) => push_bounded(resolved, text),
        RawTextComponent::Sequence(children) => {
            for child in children.iter() {
                resolve_component(child, resolver, resolved, depth + 1);
            }
        }
        RawTextComponent::Selector(selector) => match (resolver.selector)(selector) {
            Some(text) => push_bounded(resolved, &text),
            None => {
                resolved.skipped_selectors = resolved.skipped_selectors.saturating_add(1);
            }
        },
        RawTextComponent::Score { name, objective } => {
            let owner = if name.as_ref() == "*" {
                resolver.reader_name
            } else {
                name.as_ref()
            };
            match (resolver.score)(owner, objective) {
                Some(value) => push_bounded(resolved, &value.to_string()),
                None => {
                    resolved.unresolved_scores = resolved.unresolved_scores.saturating_add(1);
                }
            }
        }
        RawTextComponent::Translate { key, with } => {
            match (resolver.translate)(key) {
                Some(template) => {
                    // Arguments resolve first so positional substitution can
                    // splice them; each argument is itself budget-bounded.
                    let arguments: Vec<String> = with
                        .iter()
                        .map(|argument| {
                            let mut nested = ResolvedRawText::default();
                            resolve_component(argument, resolver, &mut nested, depth + 1);
                            resolved.unknown_translations = resolved
                                .unknown_translations
                                .saturating_add(nested.unknown_translations);
                            resolved.unresolved_scores = resolved
                                .unresolved_scores
                                .saturating_add(nested.unresolved_scores);
                            resolved.skipped_selectors = resolved
                                .skipped_selectors
                                .saturating_add(nested.skipped_selectors);
                            resolved.truncated |= nested.truncated;
                            nested.text
                        })
                        .collect();
                    let formatted = format_translation(&template, &arguments);
                    push_bounded(resolved, &formatted);
                }
                None => {
                    // The vanilla client presents an unknown key verbatim.
                    resolved.unknown_translations = resolved.unknown_translations.saturating_add(1);
                    push_bounded(resolved, key);
                }
            }
        }
    }
}

/// Substitutes `%s`/`%d` sequentially, `%1` / `%1$s` positionally, and the
/// fixed-precision `%.Nf` form — the argument families the pinned Bedrock
/// language files use (`en_US.lang` carries five `%.2f` templates). `%%`
/// escapes one percent sign; an out-of-range reference keeps its literal
/// form, and a non-numeric argument for `%.Nf` presents verbatim.
fn format_translation(template: &str, arguments: &[String]) -> String {
    let mut output = String::with_capacity(template.len());
    let mut sequential = 0usize;
    let mut chars = template.char_indices().peekable();
    while let Some((_, current)) = chars.next() {
        if current != '%' {
            output.push(current);
            continue;
        }
        match chars.peek().copied() {
            Some((_, '%')) => {
                chars.next();
                output.push('%');
            }
            Some((_, 's' | 'd')) => {
                chars.next();
                if let Some(argument) = arguments.get(sequential) {
                    output.push_str(argument);
                }
                sequential += 1;
            }
            Some((_, '.')) => {
                chars.next();
                match (chars.peek().copied(), {
                    let mut lookahead = chars.clone();
                    lookahead.next();
                    lookahead.peek().copied()
                }) {
                    (Some((_, precision @ '0'..='9')), Some((_, 'f'))) => {
                        chars.next();
                        chars.next();
                        let precision = precision as usize - '0' as usize;
                        if let Some(argument) = arguments.get(sequential) {
                            match argument.trim().parse::<f64>() {
                                Ok(value) if value.is_finite() => {
                                    output.push_str(&format!("{value:.precision$}"));
                                }
                                _ => output.push_str(argument),
                            }
                        }
                        sequential += 1;
                    }
                    _ => {
                        output.push('%');
                        output.push('.');
                    }
                }
            }
            Some((_, digit @ '1'..='9')) => {
                chars.next();
                let index = digit as usize - '1' as usize;
                // Optional Java-style `$s` suffix after the position.
                if let Some((_, '$')) = chars.peek().copied() {
                    chars.next();
                    if let Some((_, 's' | 'd')) = chars.peek().copied() {
                        chars.next();
                    }
                }
                if let Some(argument) = arguments.get(index) {
                    output.push_str(argument);
                } else {
                    output.push('%');
                    output.push(digit);
                }
            }
            _ => output.push('%'),
        }
    }
    output
}

fn push_bounded(resolved: &mut ResolvedRawText, text: &str) {
    let remaining = MAX_RAW_TEXT_OUTPUT_BYTES.saturating_sub(resolved.text.len());
    if text.len() <= remaining {
        resolved.text.push_str(text);
        return;
    }
    let mut end = remaining;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    resolved.text.push_str(&text[..end]);
    resolved.truncated = true;
}

pub fn parse_raw_text(value: &str) -> Result<Arc<RawTextDocument>, UiPacketError> {
    if value.len() > MAX_RAW_TEXT_INPUT_BYTES {
        return Err(UiPacketError::RawTextInputTooLarge {
            bytes: value.len(),
            max: MAX_RAW_TEXT_INPUT_BYTES,
        });
    }
    let wire =
        serde_json::from_str::<WireDocument>(value).map_err(|_| UiPacketError::InvalidRawText)?;
    let mut budget = ParseBudget::default();
    let components = convert_document(wire, 1, true, true, &mut budget)?;
    Ok(Arc::new(RawTextDocument {
        components: Arc::from(components),
        literal_text: Arc::from(budget.output),
        resolution: if budget.has_unresolved_components {
            RawTextResolution::RequiresResolver
        } else {
            RawTextResolution::LiteralOnly
        },
    }))
}

pub(crate) fn parse_raw_text_envelope(
    value: &str,
) -> Result<Option<Arc<RawTextDocument>>, UiPacketError> {
    // Packet validation and `bounded_text` reject oversized UI strings. Avoid doing an
    // additional semantic JSON pass over input outside that shared protocol bound.
    if value.len() > MAX_RAW_TEXT_INPUT_BYTES || !starts_with_json_object(value) {
        return Ok(None);
    }

    let has_raw_text_member = Cell::new(false);
    let mut deserializer = serde_json::Deserializer::from_str(value);
    let probe_result = RawTextEnvelopeProbe {
        has_raw_text_member: &has_raw_text_member,
    }
    .deserialize(&mut deserializer)
    .and_then(|()| deserializer.end());

    let has_raw_text_intent = has_raw_text_member.get()
        || (probe_result.is_err() && has_top_level_raw_text_member_fallback(value));
    if has_raw_text_intent {
        // Reparse with the strict schema so duplicate/unknown members, malformed values,
        // and all rawtext resource limits fail closed instead of becoming visible JSON.
        return parse_raw_text(value).map(Some);
    }

    // Malformed JSON without a semantically decoded top-level `rawtext` member is still
    // ordinary user/server text and must be preserved byte-for-byte.
    Ok(None)
}

fn starts_with_json_object(value: &str) -> bool {
    value
        .bytes()
        .find(|byte| !matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
        == Some(b'{')
}

/// Recovers top-level member boundaries after the semantic probe stops on malformed input.
///
/// This is deliberately not a permissive JSON parser. It only decodes strings in object-key
/// position and structurally skips preceding values. The packet byte limit bounds its scan and
/// nesting stack; unterminated strings or mismatched containers stop classification.
fn has_top_level_raw_text_member_fallback(value: &str) -> bool {
    let bytes = value.as_bytes();
    let Some(mut cursor) = bytes.iter().position(|byte| !is_json_whitespace(*byte)) else {
        return false;
    };
    if bytes[cursor] != b'{' {
        return false;
    }
    cursor += 1;

    loop {
        skip_json_whitespace(bytes, &mut cursor);
        if cursor >= bytes.len() || bytes[cursor] == b'}' || bytes[cursor] != b'"' {
            return false;
        }

        let key_start = cursor;
        let Some(key_end) = json_string_end(bytes, key_start) else {
            return false;
        };
        cursor = key_end;
        skip_json_whitespace(bytes, &mut cursor);
        if cursor >= bytes.len() || bytes[cursor] != b':' {
            return false;
        }

        let is_raw_text_key = serde_json::from_str::<String>(&value[key_start..key_end])
            .is_ok_and(|key| key == "rawtext");
        if is_raw_text_key {
            return true;
        }

        cursor += 1;
        let Some((boundary, delimiter)) = top_level_member_boundary(bytes, cursor) else {
            return false;
        };
        if delimiter == b'}' {
            return false;
        }
        cursor = boundary + 1;
    }
}

fn top_level_member_boundary(bytes: &[u8], mut cursor: usize) -> Option<(usize, u8)> {
    let mut closing_delimiters = Vec::new();
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'"' => cursor = json_string_end(bytes, cursor)?,
            b'{' => {
                closing_delimiters.push(b'}');
                cursor += 1;
            }
            b'[' => {
                closing_delimiters.push(b']');
                cursor += 1;
            }
            b'}' | b']' if closing_delimiters.last() == Some(&bytes[cursor]) => {
                closing_delimiters.pop();
                cursor += 1;
            }
            b',' | b'}' if closing_delimiters.is_empty() => {
                return Some((cursor, bytes[cursor]));
            }
            b'}' | b']' => return None,
            _ => cursor += 1,
        }
    }
    None
}

fn json_string_end(bytes: &[u8], start: usize) -> Option<usize> {
    if bytes.get(start) != Some(&b'"') {
        return None;
    }
    let mut cursor = start + 1;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'"' => return Some(cursor + 1),
            b'\\' => cursor = cursor.checked_add(2)?,
            0x00..=0x1f => return None,
            _ => cursor += 1,
        }
    }
    None
}

fn skip_json_whitespace(bytes: &[u8], cursor: &mut usize) {
    while bytes
        .get(*cursor)
        .is_some_and(|byte| is_json_whitespace(*byte))
    {
        *cursor += 1;
    }
}

fn is_json_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n')
}

struct RawTextEnvelopeProbe<'a> {
    has_raw_text_member: &'a Cell<bool>,
}

impl<'de> DeserializeSeed<'de> for RawTextEnvelopeProbe<'_> {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for RawTextEnvelopeProbe<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a top-level JSON object")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        while let Some(key) = map.next_key::<String>()? {
            if key == "rawtext" {
                self.has_raw_text_member.set(true);
            }
            map.next_value::<IgnoredAny>()?;
        }
        Ok(())
    }
}

#[derive(Default)]
struct ParseBudget {
    nodes: usize,
    components: usize,
    output: String,
    has_unresolved_components: bool,
}

impl ParseBudget {
    fn node(&mut self) -> Result<(), UiPacketError> {
        self.nodes = self.nodes.saturating_add(1);
        if self.nodes > MAX_RAW_TEXT_NODES {
            return Err(UiPacketError::RawTextNodeLimitExceeded {
                count: self.nodes,
                max: MAX_RAW_TEXT_NODES,
            });
        }
        Ok(())
    }

    fn component(&mut self) -> Result<(), UiPacketError> {
        self.components = self.components.saturating_add(1);
        if self.components > MAX_RAW_TEXT_COMPONENTS {
            return Err(UiPacketError::RawTextComponentLimitExceeded {
                count: self.components,
                max: MAX_RAW_TEXT_COMPONENTS,
            });
        }
        Ok(())
    }

    fn append(&mut self, value: &str) -> Result<(), UiPacketError> {
        let bytes = self.output.len().saturating_add(value.len());
        if bytes > MAX_RAW_TEXT_OUTPUT_BYTES {
            return Err(UiPacketError::RawTextOutputTooLarge {
                bytes,
                max: MAX_RAW_TEXT_OUTPUT_BYTES,
            });
        }
        self.output.push_str(value);
        Ok(())
    }
}

fn convert_document(
    wire: WireDocument,
    depth: usize,
    emit_literal: bool,
    count_object: bool,
    budget: &mut ParseBudget,
) -> Result<Vec<RawTextComponent>, UiPacketError> {
    if depth > MAX_RAW_TEXT_DEPTH {
        return Err(UiPacketError::RawTextDepthExceeded {
            depth,
            max: MAX_RAW_TEXT_DEPTH,
        });
    }
    if count_object {
        budget.node()?;
    }
    budget.node()?;
    wire.rawtext
        .into_iter()
        .map(|component| convert_component(component, depth, emit_literal, budget))
        .collect()
}

fn convert_component(
    wire: WireComponent,
    depth: usize,
    emit_literal: bool,
    budget: &mut ParseBudget,
) -> Result<RawTextComponent, UiPacketError> {
    budget.component()?;
    budget.node()?;
    match wire {
        WireComponent::Text(WireText { text }) => {
            budget.node()?;
            if emit_literal {
                budget.append(&text)?;
            }
            Ok(RawTextComponent::Text(Arc::from(text)))
        }
        WireComponent::Translate(WireTranslate { translate, with }) => {
            budget.has_unresolved_components = true;
            budget.node()?;
            let with = convert_with(with, depth.saturating_add(1), budget)?;
            Ok(RawTextComponent::Translate {
                key: Arc::from(translate),
                with: Arc::from(with),
            })
        }
        WireComponent::Score(WireScoreComponent { score }) => {
            budget.has_unresolved_components = true;
            budget.node()?;
            budget.node()?;
            budget.node()?;
            Ok(RawTextComponent::Score {
                name: Arc::from(score.name),
                objective: Arc::from(score.objective),
            })
        }
        WireComponent::Selector(WireSelector { selector }) => {
            budget.has_unresolved_components = true;
            budget.node()?;
            Ok(RawTextComponent::Selector(Arc::from(selector)))
        }
        WireComponent::Sequence(document) => {
            Ok(RawTextComponent::Sequence(Arc::from(convert_document(
                document,
                depth.saturating_add(1),
                emit_literal,
                false,
                budget,
            )?)))
        }
    }
}

fn convert_with(
    wire: Option<WireWith>,
    depth: usize,
    budget: &mut ParseBudget,
) -> Result<Vec<RawTextComponent>, UiPacketError> {
    let Some(wire) = wire else {
        return Ok(Vec::new());
    };
    if depth > MAX_RAW_TEXT_DEPTH {
        return Err(UiPacketError::RawTextDepthExceeded {
            depth,
            max: MAX_RAW_TEXT_DEPTH,
        });
    }
    match wire {
        WireWith::List(arguments) => {
            budget.node()?;
            arguments
                .into_iter()
                .map(|argument| match argument {
                    WireArgument::Text(value) => {
                        budget.component()?;
                        budget.node()?;
                        Ok(RawTextComponent::Text(Arc::from(value)))
                    }
                    WireArgument::Component(component) => {
                        convert_component(component, depth, false, budget)
                    }
                })
                .collect()
        }
        // A `with` document contributes one placeholder argument per child
        // component, exactly like the list form; wrapping the children in a
        // single sequence would collapse them into `%1` alone.
        WireWith::Document(document) => convert_document(document, depth, false, true, budget),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireDocument {
    rawtext: Vec<WireComponent>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum WireComponent {
    Text(WireText),
    Translate(WireTranslate),
    Score(WireScoreComponent),
    Selector(WireSelector),
    Sequence(WireDocument),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireText {
    text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireTranslate {
    translate: String,
    #[serde(default, deserialize_with = "deserialize_optional_with")]
    with: Option<WireWith>,
}

fn deserialize_optional_with<'de, D>(deserializer: D) -> Result<Option<WireWith>, D::Error>
where
    D: Deserializer<'de>,
{
    WireWith::deserialize(deserializer).map(Some)
}

#[derive(Deserialize)]
#[serde(untagged)]
enum WireWith {
    List(Vec<WireArgument>),
    Document(WireDocument),
}

#[derive(Deserialize)]
#[serde(untagged)]
enum WireArgument {
    Text(String),
    Component(WireComponent),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireScoreComponent {
    score: WireScore,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireScore {
    name: String,
    objective: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireSelector {
    selector: String,
}
