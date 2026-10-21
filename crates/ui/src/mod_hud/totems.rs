//! Native player-counter annotations shared by nametags and the roster.

use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Presentation of confirmed activations; enabling this policy starts a native counting interval.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TotemCounterStyle {
    pub show_self: bool,
    pub show_tab: bool,
    pub flip: bool,
    pub count_color: bool,
    pub show_nametag: bool,
    pub show_prefix: bool,
    pub separate_nametag: bool,
    pub render_first: bool,
    pub prefix_color: [f32; 4],
}

impl Default for TotemCounterStyle {
    /// Uses the published default visibility, count and prefix controls.
    fn default() -> Self {
        Self {
            show_self: true,
            show_tab: true,
            flip: false,
            count_color: true,
            show_nametag: true,
            show_prefix: false,
            separate_nametag: false,
            render_first: false,
            prefix_color: [1., 1., 85. / 255., 1.],
        }
    }
}

/// Separately colored literal runs preserve a server name's formatting and the counter's full RGBA tint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CounterAnnotation {
    pub prefix: Arc<str>,
    pub value: Arc<str>,
    pub prefix_color: [u8; 4],
    pub value_color: [u8; 4],
    pub separate: bool,
    pub before: bool,
}

impl TotemCounterStyle {
    /// Bounds the independently configurable prefix tint before native text rendering.
    pub fn validate(&self) -> Result<(), String> {
        super::color(self.prefix_color)
    }

    /// Builds one positive-use annotation; zero counts leave the ordinary name untouched.
    pub fn annotation(&self, count: u32) -> Option<CounterAnnotation> {
        if count == 0 {
            return None;
        }
        Some(CounterAnnotation {
            prefix: if self.show_prefix {
                "| ".into()
            } else {
                "".into()
            },
            value: self.count_text(count).into(),
            prefix_color: self.prefix_color.map(|v| (v * 255.).round() as u8),
            value_color: self.count_rgba(count),
            separate: self.separate_nametag,
            before: self.render_first,
        })
    }

    /// Uses the same signed count on a HUD value and on player annotations.
    pub fn count_text(&self, count: u32) -> String {
        if self.flip || count == 0 {
            count.to_string()
        } else {
            format!("-{count}")
        }
    }

    /// Resolves the count tint independently of prefix alpha or server-authored name formatting.
    pub fn count_rgba(&self, count: u32) -> [u8; 4] {
        let color = if !self.count_color {
            crate::BedrockColor::White
        } else if count <= 2 {
            crate::BedrockColor::Green
        } else if count <= 4 {
            crate::BedrockColor::Yellow
        } else {
            crate::BedrockColor::Red
        };
        let [r, g, b] = color.rgb().expect("counter palette");
        [r, g, b, 255]
    }
}
