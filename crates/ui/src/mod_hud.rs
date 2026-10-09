//! Bounded cosmetic HUD data rendered by the host's JSON-UI engine.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const MAX_HUD_BYTES: usize = 128 * 1024;
pub const MAX_HUD_CARDS: usize = 8;
pub const MAX_HUD_ROWS: usize = 64;
pub const MAX_CARD_ROWS: usize = 16;
pub const MAX_TEXT_BYTES: usize = 96;
pub const MAX_CROSSHAIR_BYTES: usize = 1024;
pub const DEFAULT_CARD_WIDTH: f32 = 148.;
pub const DEFAULT_ROW_HEIGHT: f32 = 20.;
pub const DEFAULT_ICON_SIZE: f32 = 16.;
pub const DEFAULT_TEXT_SCALE: f32 = 1.;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Anchor {
    #[default]
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// Selects a bounded row arrangement without changing its cosmetic data.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RowLayout {
    #[default]
    Standard,
    /// Places an optional icon left of the label and value on separate lines.
    StackedText,
    /// Places an optional icon at the right edge, after the inline text.
    IconRight,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hud {
    pub cards: Vec<Card>,
    /// Editor requests persist completed gestures when explicitly enabled.
    #[serde(default)]
    pub autosave: bool,
    /// Hides editor labels while preserving native draggable outlines.
    #[serde(default)]
    pub hide_editor_labels: bool,
    /// Optional client-authored JSON-UI chrome for a host-owned layout editor.
    #[serde(default)]
    pub surface: Option<crate::mod_panel::Surface>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Card {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub editor_label: String,
    #[serde(default)]
    pub anchor: Anchor,
    #[serde(default)]
    pub offset: [f32; 2],
    #[serde(default = "unit")]
    pub scale: f32,
    /// Fractions of available travel (viewport minus the scaled card size).
    #[serde(default)]
    pub position: Option<[f32; 2]>,
    #[serde(default = "background_opacity")]
    pub background_opacity: f32,
    #[serde(default)]
    pub row_layout: RowLayout,
    /// Unscaled GUI-pixel width, bounded to 48 through 512.
    #[serde(default = "card_width")]
    pub width: f32,
    /// Unscaled GUI-pixel row height, bounded to 12 through 64.
    #[serde(default = "row_height")]
    pub row_height: f32,
    /// Square icon size, bounded to 4 through 48 and to the row's height.
    #[serde(default = "icon_size")]
    pub icon_size: f32,
    /// Row text multiplier, bounded to 0.5 through 2 independently of card geometry.
    #[serde(default = "text_scale")]
    pub text_scale: f32,
    /// Optional factory placement used only by the host-owned layout editor.
    #[serde(default)]
    pub reset_anchor: Option<Anchor>,
    #[serde(default)]
    pub reset_offset: Option<[f32; 2]>,
    pub rows: Vec<Row>,
}

impl Default for Card {
    fn default() -> Self {
        Self {
            id: String::new(),
            title: String::new(),
            editor_label: String::new(),
            anchor: Anchor::default(),
            offset: [0.; 2],
            scale: unit(),
            position: None,
            background_opacity: background_opacity(),
            row_layout: RowLayout::default(),
            width: card_width(),
            row_height: row_height(),
            icon_size: icon_size(),
            text_scale: text_scale(),
            reset_anchor: None,
            reset_offset: None,
            rows: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Placement {
    pub id: String,
    pub position: Option<[f32; 2]>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EditorResult {
    pub saved: bool,
    pub reset: bool,
    pub placements: Vec<Placement>,
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
/// Preserves the existing card surface alpha when the new field is omitted.
fn background_opacity() -> f32 {
    0.82
}
/// Keeps omitted dimensions identical to the original host-owned card template.
fn card_width() -> f32 {
    DEFAULT_CARD_WIDTH
}
/// Preserves the original row spacing for existing extension payloads.
fn row_height() -> f32 {
    DEFAULT_ROW_HEIGHT
}
/// Preserves the original item and effect icon size for existing payloads.
fn icon_size() -> f32 {
    DEFAULT_ICON_SIZE
}
/// Leaves existing row fonts unchanged when the multiplier is omitted.
fn text_scale() -> f32 {
    DEFAULT_TEXT_SCALE
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
        if let Some(surface) = &self.surface {
            surface.validate()?;
            for action in surface.actions()? {
                let indexed = |prefix: &str, maximum: usize| {
                    action
                        .strip_prefix(prefix)
                        .and_then(|index| index.parse::<usize>().ok())
                        .is_some_and(|index| index < maximum)
                };
                if !matches!(
                    action.as_str(),
                    "hud.save" | "hud.cancel" | "hud.close" | "hud.reset" | "hud.grid"
                ) && !indexed("hud.card:", MAX_HUD_CARDS)
                    && !indexed("hud.done:", crate::mod_panel::MAX_PANEL_CONTROLS)
                {
                    return Err("HUD editor surface action is unsupported".into());
                }
            }
        }
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
            text(&card.editor_label)?;
            if !card.scale.is_finite()
                || !(0.5..=2.).contains(&card.scale)
                || card
                    .offset
                    .into_iter()
                    .any(|v| !v.is_finite() || v.abs() > 2048.)
                || card.rows.len() > MAX_CARD_ROWS
                || !card.background_opacity.is_finite()
                || !(0. ..=1.).contains(&card.background_opacity)
                || !card.width.is_finite()
                || !(48. ..=512.).contains(&card.width)
                || !card.row_height.is_finite()
                || !(12. ..=64.).contains(&card.row_height)
                || !card.icon_size.is_finite()
                || !(4. ..=48.).contains(&card.icon_size)
                || card.icon_size > card.row_height
                || card.icon_size > card.width - 12.
                || !card.text_scale.is_finite()
                || !(0.5..=2.).contains(&card.text_scale)
                || card.position.is_some_and(|p| {
                    p.into_iter()
                        .any(|v| !v.is_finite() || !(0. ..=1.).contains(&v))
                })
                || card
                    .reset_offset
                    .is_some_and(|p| p.into_iter().any(|v| !v.is_finite() || v.abs() > 2048.))
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
impl EditorResult {
    /// Rejects malformed placement output and changes attached to cancellation.
    pub fn validate(&self) -> Result<(), String> {
        if !self.saved && (self.reset || !self.placements.is_empty()) {
            return Err("cancelled HUD editor cannot carry changes".into());
        }
        let hud = Hud {
            cards: self
                .placements
                .iter()
                .map(|p| Card {
                    id: p.id.clone(),
                    position: p.position,
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        hud.validate()
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
    fn card_defaults_preserve_legacy_background_and_validate_new_presentation_fields() {
        let mut hud: Hud =
            serde_json::from_str(r#"{"cards":[{"id":"supplies","rows":[]}]}"#).unwrap();
        assert!(hud.cards[0].title.is_empty());
        assert_eq!(hud.cards[0].position, None);
        assert_eq!(hud.cards[0].background_opacity, 0.82);
        assert_eq!(hud.cards[0].row_layout, RowLayout::Standard);
        assert_eq!(hud.cards[0].width, DEFAULT_CARD_WIDTH);
        assert_eq!(hud.cards[0].row_height, DEFAULT_ROW_HEIGHT);
        assert_eq!(hud.cards[0].icon_size, DEFAULT_ICON_SIZE);
        assert_eq!(hud.cards[0].text_scale, DEFAULT_TEXT_SCALE);
        assert!(hud.validate().is_ok());
        for opacity in [0., 0.35, 1.] {
            hud.cards[0].background_opacity = opacity;
            assert!(hud.validate().is_ok());
        }
        for opacity in [f32::NAN, -0.01, 1.01] {
            hud.cards[0].background_opacity = opacity;
            assert!(hud.validate().is_err());
        }
        hud.cards[0].background_opacity = 0.82;
        for position in [[f32::NAN, 0.], [-0.01, 0.], [1.01, 0.], [0., f32::INFINITY]] {
            hud.cards[0].position = Some(position);
            assert!(hud.validate().is_err());
        }
        for position in [[0., 0.], [0.5, 0.5], [1., 1.]] {
            hud.cards[0].position = Some(position);
            assert!(hud.validate().is_ok());
        }
        hud.cards[0].editor_label = "Bad\nlabel".into();
        assert!(hud.validate().is_err());
    }
    #[test]
    fn row_layout_dimensions_are_bounded_and_icons_fit_their_rows() {
        for layout in ["standard", "stacked_text", "icon_right"] {
            let mut hud: Hud = serde_json::from_value(serde_json::json!({"cards":[{
                "id":"fixture","row_layout":layout,"width":64.,
                "row_height":32.,"icon_size":24.,"rows":[]
            }]}))
            .unwrap();
            assert!(hud.validate().is_ok());
            for dimensions in [
                [f32::NAN, 32., 24.],
                [47., 32., 24.],
                [513., 32., 24.],
                [64., f32::INFINITY, 24.],
                [64., 11., 4.],
                [64., 65., 24.],
                [64., 32., f32::NAN],
                [64., 32., 3.],
                [64., 64., 49.],
                [64., 20., 24.],
                [48., 48., 40.],
            ] {
                hud.cards[0].width = dimensions[0];
                hud.cards[0].row_height = dimensions[1];
                hud.cards[0].icon_size = dimensions[2];
                assert!(
                    hud.validate().is_err(),
                    "layout={layout}, dimensions={dimensions:?}"
                );
            }
        }
        assert!(
            serde_json::from_str::<Hud>(
                r#"{"cards":[{"id":"fixture","row_layout":"unknown","rows":[]}]}"#
            )
            .is_err()
        );
    }
    #[test]
    fn row_text_scale_is_optional_and_bounded_independently_of_geometry() {
        let mut hud: Hud =
            serde_json::from_str(r#"{"cards":[{"id":"fixture","rows":[]}]}"#).unwrap();
        assert_eq!(hud.cards[0].text_scale, 1.);
        for scale in [0.5, 1., 1.75, 2.] {
            hud.cards[0].text_scale = scale;
            assert!(hud.validate().is_ok());
        }
        for scale in [f32::NAN, f32::INFINITY, 0.49, 2.01] {
            hud.cards[0].text_scale = scale;
            assert!(hud.validate().is_err(), "text_scale={scale}");
        }
        let explicit: Hud =
            serde_json::from_str(r#"{"cards":[{"id":"fixture","text_scale":1.75,"rows":[]}]}"#)
                .unwrap();
        assert_eq!(explicit.cards[0].text_scale, 1.75);
        assert!(explicit.validate().is_ok());
    }
    #[test]
    fn editor_results_are_bounded_and_cancellation_carries_no_changes() {
        assert!(EditorResult::default().validate().is_ok());
        let saved = EditorResult {
            saved: true,
            reset: false,
            placements: vec![Placement {
                id: "equipment".into(),
                position: Some([0.5, 0.5]),
            }],
        };
        assert!(saved.validate().is_ok());
        assert!(
            EditorResult {
                saved: false,
                ..saved.clone()
            }
            .validate()
            .is_err()
        );
        let mut duplicate = saved.clone();
        duplicate.placements.push(duplicate.placements[0].clone());
        assert!(duplicate.validate().is_err());
        let mut excess = saved;
        excess.placements[0].position = Some([1.01, 0.]);
        assert!(excess.validate().is_err());
    }
    #[test]
    fn editor_autosave_is_opt_in_and_custom_actions_remain_bounded() {
        let mut hud: Hud = serde_json::from_str(r#"{"cards":[]}"#).unwrap();
        assert!(!hud.autosave && hud.surface.is_none());
        for (action, valid) in [
            ("hud.close", true),
            ("hud.done:0", true),
            ("hud.done:64", false),
            ("hud.card:7", true),
            ("hud.card:8", false),
            ("mod.control:0", false),
            ("button.resume_game", false),
        ] {
            hud.surface = Some(crate::mod_panel::Surface {
                screen: "fixture.editor".into(),
                document: serde_json::json!({"namespace":"fixture","editor":{
                    "type":"button","button_mappings":[{"from_button_id":"button.menu_select",
                        "to_button_id":action,"mapping_type":"pressed"}]
                }})
                .to_string(),
                bindings: Default::default(),
            });
            assert_eq!(hud.validate().is_ok(), valid, "route {action}");
        }
    }
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
            ..Default::default()
        };
        let mut hud = Hud {
            cards: vec![card],
            ..Default::default()
        };
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
