//! Bounded cosmetic HUD data rendered by the host's JSON-UI engine.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const MAX_HUD_BYTES: usize = 32 * 1024;
pub const MAX_HUD_CARDS: usize = 8;
pub const MAX_HUD_ROWS: usize = 64;
pub const MAX_CARD_ROWS: usize = 16;
pub const MAX_TEXT_BYTES: usize = 96;
pub const MAX_CROSSHAIR_BYTES: usize = 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Anchor {
    #[default]
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hud {
    pub cards: Vec<Card>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Card {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub anchor: Anchor,
    #[serde(default)]
    pub offset: [f32; 2],
    #[serde(default = "unit")]
    pub scale: f32,
    pub rows: Vec<Row>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub label: String,
    pub value: String,
    #[serde(default)]
    pub item: Option<String>,
    /// Canonical Bedrock effect icon; mutually exclusive with an item icon.
    #[serde(default)]
    pub effect_id: Option<i32>,
    #[serde(default)]
    pub metadata: u32,
    #[serde(default)]
    pub progress: Option<f32>,
    #[serde(default = "white")]
    pub color: [f32; 4],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrosshairShape {
    #[default]
    Cross,
    Dot,
    Circle,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Crosshair {
    pub shape: CrosshairShape,
    /// Arm length for a cross; radius for a dot or circle, in GUI pixels.
    pub size: f32,
    /// Empty distance from the center to the start of each cross arm.
    pub gap: f32,
    pub thickness: f32,
    pub color: [f32; 4],
    pub outline: f32,
    pub outline_color: [f32; 4],
}

impl Default for Crosshair {
    fn default() -> Self {
        Self {
            shape: CrosshairShape::Cross,
            size: 4.,
            gap: 2.,
            thickness: 1.,
            color: white(),
            outline: 1.,
            outline_color: [0., 0., 0., 1.],
        }
    }
}

fn unit() -> f32 {
    1.
}
fn white() -> [f32; 4] {
    [1.; 4]
}
fn text(value: &str) -> Result<(), String> {
    if value.len() > MAX_TEXT_BYTES || value.chars().any(|c| c.is_control() || c == '§') {
        return Err("HUD text must be bounded plain single-line text".into());
    }
    Ok(())
}
fn color(value: [f32; 4]) -> Result<(), String> {
    if value
        .into_iter()
        .any(|v| !v.is_finite() || !(0. ..=1.).contains(&v))
    {
        return Err("HUD colors must contain finite values between zero and one".into());
    }
    Ok(())
}
impl Hud {
    /// Rejects unbounded or malformed guest data before publication.
    pub fn validate(&self) -> Result<(), String> {
        if self.cards.len() > MAX_HUD_CARDS
            || self.cards.iter().map(|c| c.rows.len()).sum::<usize>() > MAX_HUD_ROWS
        {
            return Err("HUD has too many cards or rows".into());
        }
        let mut ids = HashSet::new();
        for card in &self.cards {
            if card.id.is_empty()
                || card.id.len() > 48
                || !card
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
                || !ids.insert(&card.id)
            {
                return Err("HUD cards require bounded unique identifiers".into());
            }
            text(&card.title)?;
            if !card.scale.is_finite()
                || !(0.5..=2.).contains(&card.scale)
                || card
                    .offset
                    .into_iter()
                    .any(|v| !v.is_finite() || v.abs() > 2048.)
                || card.rows.len() > MAX_CARD_ROWS
            {
                return Err("HUD card geometry exceeds its bounds".into());
            }
            for row in &card.rows {
                text(&row.label)?;
                text(&row.value)?;
                color(row.color)?;
                if row
                    .progress
                    .is_some_and(|v| !v.is_finite() || !(0. ..=1.).contains(&v))
                {
                    return Err("HUD progress must be between zero and one".into());
                }
                if row.item.is_some() && row.effect_id.is_some()
                    || row.effect_id.is_some_and(|id| !(1..=255).contains(&id))
                {
                    return Err("HUD rows accept either an item or a bounded effect ID".into());
                }
                if row.item.as_ref().is_some_and(|id| {
                    id.is_empty()
                        || id.len() > 128
                        || !id.bytes().all(|b| {
                            b.is_ascii_lowercase() || b.is_ascii_digit() || b"_:.-/".contains(&b)
                        })
                }) {
                    return Err("HUD item must be a bounded resource identifier".into());
                }
            }
        }
        Ok(())
    }
}
impl Crosshair {
    /// Keeps custom crosshair geometry and color within bounded cosmetic limits.
    pub fn validate(&self) -> Result<(), String> {
        for (value, min, max) in [
            (self.size, 0.5, 16.),
            (self.gap, 0., 12.),
            (self.thickness, 0.5, 8.),
            (self.outline, 0., 4.),
        ] {
            if !value.is_finite() || !(min..=max).contains(&value) {
                return Err("crosshair geometry exceeds its bounds".into());
            }
        }
        color(self.color)?;
        color(self.outline_color)?;
        if (self.color[3] * 255.).round() < 1. {
            return Err("custom crosshair must have visible color".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn crosshair_rejects_invisible_nonfinite_and_excessive_geometry() {
        for value in [f32::NAN, f32::INFINITY, 0., 17.] {
            assert!(
                Crosshair {
                    size: value,
                    ..Default::default()
                }
                .validate()
                .is_err()
            );
        }
        for alpha in [0., 0.0001, 0.49 / 255.] {
            assert!(
                Crosshair {
                    color: [1., 1., 1., alpha],
                    outline: 0.,
                    ..Default::default()
                }
                .validate()
                .is_err(),
                "foreground alpha must remain visible after 8-bit conversion"
            );
        }
        assert!(
            Crosshair {
                color: [1., 1., 1., 1. / 255.],
                ..Default::default()
            }
            .validate()
            .is_ok()
        );
        assert!(Crosshair::default().validate().is_ok());
    }
    #[test]
    fn hud_rejects_ambiguous_ids_formatting_and_invalid_progress() {
        let row = Row {
            label: "Helmet".into(),
            value: "34 / 363".into(),
            item: Some("minecraft:diamond_helmet".into()),
            effect_id: None,
            metadata: 0,
            progress: Some(0.1),
            color: white(),
        };
        let card = Card {
            id: "armor".into(),
            title: "Armor".into(),
            anchor: Anchor::TopRight,
            offset: [-6., 6.],
            scale: 1.,
            rows: vec![row],
        };
        let mut hud = Hud { cards: vec![card] };
        assert!(hud.validate().is_ok());
        hud.cards.push(hud.cards[0].clone());
        assert!(hud.validate().is_err());
        hud.cards.pop();
        hud.cards[0].rows[0].progress = Some(1.1);
        assert!(hud.validate().is_err());
        hud.cards[0].rows[0].progress = None;
        hud.cards[0].title = "§khidden".into();
        assert!(hud.validate().is_err());
    }
}
