//! Single-value text HUD styling, with native font measurements kept outside authored equality.

use super::{Card, NativeRows, RowLayout};
use serde::{Deserialize, Serialize};

/// Maximum unscaled text-plate height admitted before font measurement.
pub const MAX_TEXT_PLATE_HEIGHT: f32 = 128.;

/// Authored text presentation and its private native measurement cache.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextPlate {
    #[serde(flatten)]
    pub style: TextPlateStyle,
    /// Native-only layout; JSON cannot supply font measurements or draw coordinates.
    #[serde(skip)]
    pub layout: Option<TextPlateLayout>,
}

impl PartialEq for TextPlate {
    /// Measurements are derived from the current font, rather than an authored HUD change.
    fn eq(&self, other: &Self) -> bool {
        self.style == other.style
    }
}

/// Geometry and colors for a plain value with optional brackets.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TextPlateStyle {
    pub brackets: bool,
    pub bracket_color: [f32; 4],
    pub background: [f32; 4],
    pub border_color: [f32; 4],
    pub border_width: f32,
    /// Unscaled GUI-pixel background corner radius, 0 through 16; the card's own radius is unused.
    pub radius: f32,
    /// None fits the text and padding; Some reserves this exact background dimension.
    pub width: Option<f32>,
    pub height: Option<f32>,
}

impl Default for TextPlateStyle {
    /// Keeps unconfigured plates plain, with transparent chrome.
    fn default() -> Self {
        Self {
            brackets: false,
            bracket_color: [1.; 4],
            background: [0.; 4],
            border_color: [0.; 4],
            border_width: 0.,
            radius: 0.,
            width: None,
            height: None,
        }
    }
}

/// Measured scaled GUI rectangles for the background and the three text spans.
#[derive(Clone, Debug, PartialEq)]
pub struct TextPlateLayout {
    pub scale: f32,
    pub size: [f64; 2],
    pub background: [f64; 4],
    pub spans: [[f64; 4]; 3],
}

impl TextPlate {
    /// Bounds chrome and admits only a single plain value, without mixed layouts or artwork.
    pub fn validate(&self, card: &Card) -> Result<(), String> {
        let s = &self.style;
        for color in [s.bracket_color, s.background, s.border_color] {
            super::color(color)?;
        }
        if !s.border_width.is_finite()
            || !(0. ..=8.).contains(&s.border_width)
            || !super::background_radius_valid(s.radius)
            || s.width
                .is_some_and(|v| !v.is_finite() || !(1. ..=super::MAX_CARD_WIDTH).contains(&v))
            || s.height
                .is_some_and(|v| !v.is_finite() || !(1. ..=MAX_TEXT_PLATE_HEIGHT).contains(&v))
            || !card.title.is_empty()
            || card.compass.is_some()
            || !card.cells.is_empty()
            || card.row_layout != RowLayout::Standard
            || card.rows.len() > 1
            || card.rows.iter().any(|r| {
                !r.label.is_empty()
                    || r.formatted
                    || r.localized
                    || r.item.is_some()
                    || r.effect_id.is_some()
                    || r.progress.is_some()
            })
            || card.source.as_ref().is_some_and(|source| {
                source.limit != 1
                    || source.brackets
                    || match &source.kind {
                        NativeRows::Clock { .. }
                        | NativeRows::TotemUses { .. }
                        | NativeRows::WorldDays => false,
                        NativeRows::ItemCount { label, .. } => !label.is_empty(),
                        _ => true,
                    }
            })
        {
            return Err("text plate requires bounded chrome and one plain value".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Native count plates remain one literal span, without hidden labels, extra rows or duplicate brackets.
    #[test]
    fn counter_plates_admit_only_single_plain_native_rows() {
        let source = super::super::RowSource {
            kind: NativeRows::ItemCount {
                label: String::new(),
                items: Vec::new(),
                blocks: true,
                potion_effect: None,
            },
            limit: 1,
            color: [1.; 4],
            brackets: false,
        };
        let mut card = Card {
            id: "count".into(),
            source: Some(source),
            text_plate: Some(TextPlate::default()),
            ..Default::default()
        };
        assert!(card.text_plate.as_ref().unwrap().validate(&card).is_ok());
        card.source.as_mut().unwrap().limit = 2;
        assert!(card.text_plate.as_ref().unwrap().validate(&card).is_err());
        card.source.as_mut().unwrap().limit = 1;
        card.source.as_mut().unwrap().brackets = true;
        assert!(card.text_plate.as_ref().unwrap().validate(&card).is_err());
        card.source.as_mut().unwrap().brackets = false;
        if let NativeRows::ItemCount { label, .. } = &mut card.source.as_mut().unwrap().kind {
            *label = "Items".into();
        }
        assert!(card.text_plate.as_ref().unwrap().validate(&card).is_err());
    }

    /// Authored fields survive round trips; cached geometry cannot be injected by JSON.
    #[test]
    fn text_plate_contract_rejects_invalid_chrome_and_native_measurements() {
        let mut card = Card {
            id: "clock".into(),
            text_plate: Some(TextPlate::default()),
            ..Default::default()
        };
        assert!(card.text_plate.as_ref().unwrap().validate(&card).is_ok());
        for width in [f32::NAN, 0., 513.] {
            card.text_plate.as_mut().unwrap().style.width = Some(width);
            assert!(card.text_plate.as_ref().unwrap().validate(&card).is_err());
        }
        assert!(serde_json::from_str::<TextPlate>(r#"{"layout":{"size":[1,2]}}"#).is_err());
        let mut a = TextPlate::default();
        let b = a.clone();
        a.layout = Some(TextPlateLayout {
            scale: 1.,
            size: [10., 10.],
            background: [0.; 4],
            spans: [[0.; 4]; 3],
        });
        assert_eq!(
            a, b,
            "native measurement must not masquerade as an authored change"
        );
        a.style.brackets = true;
        assert_ne!(a, b);
    }
}
