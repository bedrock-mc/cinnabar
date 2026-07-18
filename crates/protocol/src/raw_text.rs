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
    let _probe_result = RawTextEnvelopeProbe {
        has_raw_text_member: &has_raw_text_member,
    }
    .deserialize(&mut deserializer)
    .and_then(|()| deserializer.end());

    if has_raw_text_member.get() {
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
        WireWith::Document(document) => {
            budget.component()?;
            Ok(vec![RawTextComponent::Sequence(Arc::from(
                convert_document(document, depth, false, true, budget)?,
            ))])
        }
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
