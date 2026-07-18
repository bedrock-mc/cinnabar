use std::sync::Arc;

use serde::Deserialize;

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
    let components = convert_document(wire, 1, true, &mut budget)?;
    Ok(Arc::new(RawTextDocument {
        components: Arc::from(components),
        literal_text: Arc::from(budget.output),
    }))
}

#[derive(Default)]
struct ParseBudget {
    nodes: usize,
    components: usize,
    output: String,
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
    budget: &mut ParseBudget,
) -> Result<Vec<RawTextComponent>, UiPacketError> {
    if depth > MAX_RAW_TEXT_DEPTH {
        return Err(UiPacketError::RawTextDepthExceeded {
            depth,
            max: MAX_RAW_TEXT_DEPTH,
        });
    }
    budget.node()?;
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
            budget.node()?;
            let with = convert_with(with, depth.saturating_add(1), budget)?;
            Ok(RawTextComponent::Translate {
                key: Arc::from(translate),
                with: Arc::from(with),
            })
        }
        WireComponent::Score(WireScoreComponent { score }) => {
            budget.node()?;
            budget.node()?;
            budget.node()?;
            Ok(RawTextComponent::Score {
                name: Arc::from(score.name),
                objective: Arc::from(score.objective),
            })
        }
        WireComponent::Selector(WireSelector { selector }) => {
            budget.node()?;
            Ok(RawTextComponent::Selector(Arc::from(selector)))
        }
        WireComponent::Sequence(document) => Ok(RawTextComponent::Sequence(Arc::from(
            convert_document(document, depth.saturating_add(1), emit_literal, budget)?,
        ))),
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
    budget.node()?;
    match wire {
        WireWith::List(arguments) => arguments
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
            .collect(),
        WireWith::Document(document) => Ok(vec![RawTextComponent::Sequence(Arc::from(
            convert_document(document, depth, false, budget)?,
        ))]),
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
    #[serde(default)]
    with: Option<WireWith>,
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
