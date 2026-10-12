//! Bounded cosmetic HUD data rendered by the host's JSON-UI engine.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

mod cosmetic;
mod editor_style;
pub use cosmetic::{ActorStyle, HungerOverlay, NametagStyle};
pub use editor_style::{EditorStyle, MAX_EDITOR_HANDLE_RADIUS};

/// Bound for one read-only server text field crossing an extension callback.
pub const MAX_OBSERVED_TEXT_BYTES: usize = 256;
/// Bound for the newest received chat history imported by one callback.
pub const MAX_OBSERVED_CHAT_MESSAGES: usize = 32;
/// Maximum received messages returned by one incremental history import.
pub const MAX_CHAT_READ_MESSAGES: usize = 4;
/// Bound for camera-projected labels, separate from draggable HUD cards.
pub const MAX_WORLD_LABELS: usize = 64;
/// Bound for clickable literal spans in native chat.
pub const MAX_CHAT_ACTIONS: usize = 32;
pub const MAX_HUD_BYTES: usize = 128 * 1024;
pub const MAX_HUD_CARDS: usize = 16;
pub const MAX_HUD_ROWS: usize = 64;
pub const MAX_CARD_ROWS: usize = 16;
/// Total rectangular cells, independent of ordinary text-row limits.
pub const MAX_HUD_CELLS: usize = 128;
/// Rectangular cells in one card; their geometry remains individually validated.
pub const MAX_CARD_CELLS: usize = 64;
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
    /// Cosmetic native actor overrides; absence restores native presentation.
    #[serde(default)]
    pub actor_style: Option<ActorStyle>,
    /// Adds cosmetic food-state artwork to the actual native hunger controls.
    #[serde(default)]
    pub hunger_overlay: Option<HungerOverlay>,
    /// Suppresses the ordinary status-effect icons while this HUD is published.
    #[serde(default)]
    pub hide_effect_icons: bool,
    /// Suppresses the ordinary server sidebar while this HUD is published.
    #[serde(default)]
    pub hide_scoreboard: bool,
    /// Bounded world anchors rendered by the host through the actual gameplay camera.
    #[serde(default)]
    pub world_labels: Vec<WorldLabel>,
    /// Client-selected literal spans made clickable in the open native chat history.
    #[serde(default)]
    pub chat_actions: Vec<ChatAction>,
    /// Editor requests persist completed gestures when explicitly enabled.
    #[serde(default)]
    pub autosave: bool,
    /// Enables host-owned corner handles for uniform card resizing.
    #[serde(default)]
    pub resizable: bool,
    /// Hides editor labels while preserving native draggable outlines.
    #[serde(default)]
    pub hide_editor_labels: bool,
    /// Optional client-authored JSON-UI chrome for a host-owned layout editor.
    #[serde(default)]
    pub surface: Option<crate::mod_panel::Surface>,
    /// Optional colors for the editor's outlines, handles, guides and grid.
    #[serde(default)]
    pub editor_style: Option<EditorStyle>,
}

/// Exact plain-text chat span and the extension event emitted by a primary click.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatAction {
    pub text: String,
    pub action: String,
}

/// Cosmetic world anchor; text content and persistence belong to the extension.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorldLabel {
    pub position: [f32; 3],
    pub text: String,
    pub icon: Option<String>,
    pub icon_scale: f32,
    pub color: [f32; 4],
    pub scale: f32,
    pub padding: f32,
    pub border_width: f32,
    pub background_opacity: f32,
    pub shadow: bool,
    /// Optional camera-centered visibility cone, in degrees.
    pub near_angle: Option<f32>,
}
impl Default for WorldLabel {
    fn default() -> Self {
        Self {
            position: [0.; 3],
            text: String::new(),
            icon: None,
            icon_scale: 1.,
            color: [1.; 4],
            scale: 1.,
            padding: 4.,
            border_width: 0.,
            background_opacity: 0.35,
            shadow: true,
            near_angle: None,
        }
    }
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
    /// Unscaled GUI-pixel width: 48–512 for rows, 1–512 for cells.
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
    /// Optional factory scale restored by the layout editor.
    #[serde(default)]
    pub reset_scale: Option<f32>,
    pub rows: Vec<Row>,
    /// Optional rectangular text cells instead of linear rows.
    #[serde(default)]
    pub cells: Vec<Cell>,
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
            reset_scale: None,
            rows: Vec::new(),
            cells: Vec::new(),
        }
    }
}

/// Bounded GUI-pixel rectangle with centered text and an optional smaller second line.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Cell {
    pub rect: [f32; 4],
    pub label: String,
    pub value: String,
    pub background: [f32; 4],
    pub color: [f32; 4],
    pub shadow: bool,
    /// Allows bounded Bedrock style codes in text instead of the default plain text contract.
    pub formatted: bool,
}
impl Default for Cell {
    fn default() -> Self {
        Self {
            rect: [0., 0., 1., 1.],
            label: String::new(),
            value: String::new(),
            background: [0.; 4],
            color: white(),
            shadow: false,
            formatted: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Placement {
    pub id: String,
    pub scale: f32,
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
    /// Allows bounded Bedrock style codes in row labels and values.
    #[serde(default)]
    pub formatted: bool,
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
/// Extends the cosmetic text contract only when explicitly requested by the author.
fn styled_text(value: &str, formatted: bool) -> Result<(), String> {
    if !formatted {
        return text(value);
    }
    if value.len() > MAX_TEXT_BYTES || value.chars().any(char::is_control) {
        return Err("formatted HUD text exceeds single-line bounds".into());
    }
    crate::parse_bedrock_text(value, MAX_TEXT_BYTES).map_err(|error| error.to_string())?;
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
        cosmetic::validate(self.actor_style.as_ref(), self.hunger_overlay.as_ref())?;
        if let Some(style) = &self.editor_style {
            style.validate()?;
        }
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
        if self.chat_actions.len() > MAX_CHAT_ACTIONS
            || self.chat_actions.iter().any(|a| {
                a.text.is_empty()
                    || a.text.len() > 96
                    || a.text.chars().any(char::is_control)
                    || a.action.is_empty()
                    || a.action.len() > crate::mod_panel::MAX_PANEL_ID_BYTES
                    || !a
                        .action
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
            })
        {
            return Err("invalid chat actions".into());
        }
        if self.world_labels.len() > MAX_WORLD_LABELS
            || self.world_labels.iter().any(|label| {
                !label.position.iter().all(|v| v.is_finite())
                    || label.text.len() > 256
                    || label.text.chars().any(|c| c.is_control() && c != '\n')
                    || label
                        .icon
                        .as_ref()
                        .is_some_and(|icon| icon.len() > 32 || icon.chars().any(char::is_control))
                    || !label.icon_scale.is_finite()
                    || !(0.1..=4.0).contains(&label.icon_scale)
                    || !label
                        .color
                        .iter()
                        .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
                    || !label.scale.is_finite()
                    || !(0.1..=4.0).contains(&label.scale)
                    || !label.padding.is_finite()
                    || !(0.0..=16.0).contains(&label.padding)
                    || !label.border_width.is_finite()
                    || !(0.0..=4.0).contains(&label.border_width)
                    || !label.background_opacity.is_finite()
                    || !(0.0..=1.0).contains(&label.background_opacity)
                    || label
                        .near_angle
                        .is_some_and(|v| !v.is_finite() || !(1.0..=90.0).contains(&v))
            })
        {
            return Err("invalid world labels".into());
        }
        if self.cards.len() > MAX_HUD_CARDS
            || self.cards.iter().map(|c| c.rows.len()).sum::<usize>() > MAX_HUD_ROWS
            || self.cards.iter().map(|c| c.cells.len()).sum::<usize>() > MAX_HUD_CELLS
        {
            return Err("HUD has too many cards, rows or cells".into());
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
            if card
                .reset_scale
                .is_some_and(|scale| !scale.is_finite() || !(0.5..=2.).contains(&scale))
                || !card.scale.is_finite()
                || !(0.5..=2.).contains(&card.scale)
                || card
                    .offset
                    .into_iter()
                    .any(|v| !v.is_finite() || v.abs() > 2048.)
                || card.rows.len() > MAX_CARD_ROWS
                || card.cells.len() > MAX_CARD_CELLS
                || !card.rows.is_empty() && !card.cells.is_empty()
                || !card.background_opacity.is_finite()
                || !(0. ..=1.).contains(&card.background_opacity)
                || !card.width.is_finite()
                || !(if card.cells.is_empty() { 48. } else { 1. }..=512.).contains(&card.width)
                || !card.row_height.is_finite()
                || !(12. ..=64.).contains(&card.row_height)
                || !card.icon_size.is_finite()
                || !(4. ..=48.).contains(&card.icon_size)
                || card.cells.is_empty()
                    && (card.icon_size > card.row_height || card.icon_size > card.width - 12.)
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
            for cell in &card.cells {
                styled_text(&cell.label, cell.formatted)?;
                styled_text(&cell.value, cell.formatted)?;
                color(cell.color)?;
                color(cell.background)?;
                let [x, y, width, height] = cell.rect;
                if cell.rect.iter().any(|v| !v.is_finite())
                    || x < 0.
                    || y < 0.
                    || width <= 0.
                    || height <= 0.
                    || x + width > card.width
                    || y + height > 1024.
                {
                    return Err("HUD cell rectangle exceeds its card bounds".into());
                }
            }
            for row in &card.rows {
                styled_text(&row.label, row.formatted)?;
                styled_text(&row.value, row.formatted)?;
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
                    scale: p.scale,
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
    fn larger_card_catalog_and_actor_cosmetics_keep_explicit_bounds() {
        let mut hud:Hud=serde_json::from_value(serde_json::json!({"cards":(0..MAX_HUD_CARDS)
            .map(|index|serde_json::json!({"id":format!("card_{index}"),"rows":[]})).collect::<Vec<_>>(),
            "actor_style":{"hurt_color":[1.,0.,0.,0.3],"nametags":{"scale":1.,"shadow":true}}})).unwrap();
        assert!(hud.validate().is_ok());
        hud.actor_style
            .as_mut()
            .unwrap()
            .nametags
            .as_mut()
            .unwrap()
            .opacity = 1.01;
        assert!(hud.validate().is_err());
        hud.actor_style
            .as_mut()
            .unwrap()
            .nametags
            .as_mut()
            .unwrap()
            .opacity = 0.5;
        hud.actor_style.as_mut().unwrap().attackable_color = Some([1., 0., 0., 1.1]);
        assert!(hud.validate().is_err());
        hud.actor_style.as_mut().unwrap().attackable_color = Some([1., 0., 0., 0.3]);
        assert!(hud.validate().is_ok());
        hud.actor_style.as_mut().unwrap().hurt_color = Some([f32::NAN, 0., 0., 1.]);
        assert!(hud.validate().is_err());
        hud.actor_style = None;
        let mut excess = hud.cards[0].clone();
        excess.id = "excess".into();
        hud.cards.push(excess);
        assert!(hud.validate().is_err());
    }
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
                scale: 1.,
                position: Some([0.5, 0.5]),
            }],
        };
        assert!(saved.validate().is_ok());
        for scale in [f32::NAN, 0.49, 2.01] {
            let mut malformed = saved.clone();
            malformed.placements[0].scale = scale;
            assert!(malformed.validate().is_err());
        }
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
        let done_last = format!("hud.done:{}", crate::mod_panel::MAX_PANEL_CONTROLS - 1);
        let done_excess = format!("hud.done:{}", crate::mod_panel::MAX_PANEL_CONTROLS);
        let card_last = format!("hud.card:{}", MAX_HUD_CARDS - 1);
        let card_excess = format!("hud.card:{}", MAX_HUD_CARDS);
        for (action, valid) in [
            ("hud.close", true),
            ("hud.done:0", true),
            (done_last.as_str(), true),
            (done_excess.as_str(), false),
            (card_last.as_str(), true),
            (card_excess.as_str(), false),
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
            formatted: false,
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

#[cfg(test)]
mod cell_tests {
    use super::*;

    #[test]
    fn rectangular_cells_are_optional_and_bounded() {
        let mut hud: Hud = serde_json::from_str(r#"{"cards":[{"id":"input","width":56,"rows":[],"cells":[{"rect":[19,0,18,18],"label":"W"}]}]}"#).unwrap();
        assert!(hud.validate().is_ok());
        hud.cards[0].width = 32.;
        hud.cards[0].cells[0].rect = [11., 0., 10., 10.];
        assert!(hud.validate().is_ok());
        hud.cards[0].width = 56.;
        for rect in [
            [40., 0., 18., 18.],
            [0., 1010., 18., 18.],
            [0., 0., 0., 18.],
            [-1., 0., 18., 18.],
            [0., 0., f32::NAN, 18.],
        ] {
            hud.cards[0].cells[0].rect = rect;
            assert!(hud.validate().is_err(), "accepted {rect:?}");
        }
        hud.cards[0].cells[0].rect = [0., 0., 18., 18.];
        hud.cards[0].cells[0].background = [0., 0., 0., 2.];
        assert!(hud.validate().is_err());
        hud.cards[0].cells[0].background = [0.; 4];
        hud.cards[0].cells = vec![Cell::default(); MAX_CARD_CELLS];
        assert!(hud.validate().is_ok());
        hud.cards[0].cells.push(Cell::default());
        assert!(hud.validate().is_err());
    }
    #[test]
    fn rows_and_cells_retain_independent_card_and_total_limits() {
        let row: Row = serde_json::from_str(r#"{"label":"Observed","value":"1"}"#).unwrap();
        let row_card = Card {
            id: "rows".into(),
            rows: vec![row.clone(); MAX_CARD_ROWS],
            ..Default::default()
        };
        let mut single = Hud {
            cards: vec![row_card.clone()],
            ..Default::default()
        };
        assert!(single.validate().is_ok());
        single.cards[0].rows.push(row.clone());
        assert!(
            single.validate().is_err(),
            "the text-row limit stays unchanged"
        );
        single.cards[0].rows.pop();
        single.cards[0].cells.push(Cell::default());
        assert!(
            single.validate().is_err(),
            "rows and cells cannot share one card"
        );

        let mut hud = Hud::default();
        for index in 0..MAX_HUD_ROWS / MAX_CARD_ROWS {
            hud.cards.push(Card {
                id: format!("rows_{index}"),
                ..row_card.clone()
            });
        }
        for index in 0..MAX_HUD_CELLS / MAX_CARD_CELLS {
            hud.cards.push(Card {
                id: format!("cells_{index}"),
                cells: vec![Cell::default(); MAX_CARD_CELLS],
                ..Default::default()
            });
        }
        assert!(
            hud.validate().is_ok(),
            "full cell budgets must coexist with the existing full row budget"
        );
        hud.cards.push(Card {
            id: "extra_cell".into(),
            cells: vec![Cell::default()],
            ..Default::default()
        });
        assert!(
            hud.validate().is_err(),
            "the total cell limit remains bounded"
        );
        hud.cards.pop();
        hud.cards.push(Card {
            id: "extra_row".into(),
            rows: vec![row],
            ..Default::default()
        });
        assert!(
            hud.validate().is_err(),
            "the total row limit stays unchanged"
        );
    }
    #[test]
    fn formatted_cells_are_explicit_bounded_and_accept_native_style_codes() {
        let mut hud: Hud = serde_json::from_str(r#"{"cards":[{"id":"sidebar","rows":[],"cells":[{"label":"§aServer §lScore","formatted":true}]}]}"#).unwrap();
        assert!(hud.validate().is_ok());
        hud.cards[0].cells[0].formatted = false;
        assert!(hud.validate().is_err());
        hud.cards[0].cells[0].formatted = true;
        hud.cards[0].cells[0].label.push('\n');
        assert!(hud.validate().is_err());
    }
}
