//! Bounded cosmetic HUD data rendered by the host's JSON-UI engine.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

mod bindings;
mod geometry;
pub use bindings::{HudBindings, MAX_HUD_BINDINGS};
pub use geometry::{HudGeometry, HudGeometryRequests, MAX_HUD_GEOMETRY_REQUESTS};
mod compass;
mod cosmetic;
mod editor_style;
mod source;
mod text_plate;
pub use text_plate::{MAX_TEXT_PLATE_HEIGHT, TextPlate, TextPlateLayout, TextPlateStyle};
mod totems;
pub use totems::{CounterAnnotation, TotemCounterStyle};
mod merge;
mod update;
pub use compass::{
    COMPASS_DEFAULT_WIDTH, COMPASS_DIRECTIONS, COMPASS_HEIGHT, COMPASS_MAX_WIDTH,
    COMPASS_MIN_WIDTH, COMPASS_TICKS, Compass, CompassFrame, CompassMarker, MAX_COMPASS_COORDINATE,
    MAX_COMPASS_MARKERS, MAX_HUD_COMPASSES, compass_bearing, compass_offset,
};
pub use cosmetic::{ActorStyle, HungerOverlay, NametagStyle};
pub use editor_style::{EditorStyle, MAX_EDITOR_HANDLE_RADIUS};
pub use merge::HudMergeCache;
pub use source::{ActorBounds, ActorHealth, NativeRows, RowSource};
pub use update::HudUpdate;

/// Bound for one read-only server text field crossing an extension callback.
pub const MAX_OBSERVED_TEXT_BYTES: usize = 256;
/// Bound for the newest received chat history imported by one callback.
pub const MAX_OBSERVED_CHAT_MESSAGES: usize = 32;
/// Maximum received messages returned by one incremental history import.
pub const MAX_CHAT_READ_MESSAGES: usize = 4;
/// Bound for camera-projected labels, separate from draggable HUD cards.
pub const MAX_WORLD_LABELS: usize = 64;
/// Reserved projected labels for the host's bounded nearby-actor observation set.
pub const MAX_NATIVE_ACTOR_LABELS: usize = 16;
/// Bound for clickable literal spans in native chat.
pub const MAX_CHAT_ACTIONS: usize = 32;
pub const MAX_HUD_BYTES: usize = 128 * 1024;
pub const MAX_HUD_CARDS: usize = 64;
pub const MAX_HUD_ROWS: usize = 256;
pub const MAX_CARD_ROWS: usize = 64;
/// Total rectangular cells, independent of ordinary text-row limits.
pub const MAX_HUD_CELLS: usize = 128;
/// Rectangular cells in one card; their geometry remains individually validated.
pub const MAX_CARD_CELLS: usize = 64;
pub const MAX_TEXT_BYTES: usize = 96;
pub const MAX_CROSSHAIR_BYTES: usize = 1024;
/// Uniform HUD scales shared by admission and the native layout editor.
pub const MIN_CARD_SCALE: f32 = 0.25;
pub const MAX_CARD_SCALE: f32 = 5.;
pub const MAX_CARD_WIDTH: f32 = 512.;
pub const DEFAULT_CARD_WIDTH: f32 = 148.;
pub const DEFAULT_ROW_HEIGHT: f32 = 20.;
pub const DEFAULT_ICON_SIZE: f32 = 16.;
pub const DEFAULT_TEXT_SCALE: f32 = 1.;

/// Accepts a local card name; qualified and foreign-owner names cannot be selected.
pub fn valid_card_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 48
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Anchor {
    #[default]
    TopLeft,
    /// Keeps the card horizontally centered as its size or viewport changes.
    TopCenter,
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
    /// Places item rows in square cells, using the row height as the cell width.
    Grid,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hud {
    pub cards: Vec<Card>,
    /// Cosmetic native actor overrides; absence restores native presentation.
    #[serde(default)]
    pub actor_style: Option<ActorStyle>,
    /// Read-only replicated health labels; players and mobs require their own grants.
    #[serde(default)]
    pub actor_health: Option<ActorHealth>,
    /// Native depth-tested outlines; requires render plus the selected actor grants.
    #[serde(default)]
    pub actor_bounds: Option<ActorBounds>,
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

/// Selects host-owned typography for a draggable HUD card.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardFont {
    #[default]
    Native,
    Sans,
}

impl CardFont {
    /// Returns the same font alias for painting and native text measurement.
    pub fn name(self) -> &'static str {
        match self {
            Self::Native => "default",
            Self::Sans => crate::mod_panel::SANS_FONT_NAME,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Card {
    pub id: String,
    /// Reserves a card for panel previews without showing it during gameplay.
    #[serde(default)]
    pub preview_only: bool,
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
    /// Omitted typography keeps the native pixel font.
    #[serde(default)]
    pub font: CardFont,
    /// Draws row labels and values with a shadow; legacy cards keep their shadow.
    #[serde(default = "enabled")]
    pub shadow: bool,
    /// Optional factory placement used only by the host-owned layout editor.
    #[serde(default)]
    pub reset_anchor: Option<Anchor>,
    #[serde(default)]
    pub reset_offset: Option<[f32; 2]>,
    /// Optional factory scale restored by the layout editor.
    #[serde(default)]
    pub reset_scale: Option<f32>,
    pub rows: Vec<Row>,
    /// Optional native rows. Game-backed sources require player-state; Clock is local UI time.
    #[serde(default)]
    pub source: Option<RowSource>,
    /// A native compass uses current-session motion instead of guest-sampled geometry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compass: Option<Compass>,
    /// Native measured text with independently colored brackets, background and border.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_plate: Option<TextPlate>,
    /// Optional rectangular text cells instead of linear rows.
    #[serde(default)]
    pub cells: Vec<Cell>,
}

impl Default for Card {
    fn default() -> Self {
        Self {
            id: String::new(),
            preview_only: false,
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
            font: CardFont::default(),
            shadow: true,
            reset_anchor: None,
            reset_offset: None,
            reset_scale: None,
            rows: Vec::new(),
            source: None,
            compass: None,
            text_plate: None,
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
    /// Treats the label as a native language key; values remain literal measured text.
    #[serde(default)]
    pub localized: bool,
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
/// Preserves legacy text shadows when a card omits its preference.
fn enabled() -> bool {
    true
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
/// Matches the single resource-identifier contract used by authored rows and native filters.
fn item_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_:.-/".contains(&b))
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
        if let Some(bounds) = &self.actor_bounds {
            bounds.validate()?;
        }
        if let Some(health) = &self.actor_health {
            health.validate()?;
            if self.world_labels.len() > MAX_WORLD_LABELS - MAX_NATIVE_ACTOR_LABELS {
                return Err(
                    "actor health labels require sixteen reserved world-label slots".into(),
                );
            }
        }
        cosmetic::validate(self.actor_style.as_ref(), self.hunger_overlay.as_ref())?;
        if let Some(style) = &self.editor_style {
            style.validate()?;
        }
        if let Some(surface) = &self.surface {
            if !surface.collections.is_empty() {
                return Err("HUD editor surfaces do not support tile collections".into());
            }
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
            || self
                .cards
                .iter()
                .map(|c| {
                    c.source
                        .as_ref()
                        .map_or(c.rows.len(), |s| usize::from(s.limit))
                })
                .sum::<usize>()
                > MAX_HUD_ROWS
            || self.cards.iter().map(|c| c.cells.len()).sum::<usize>() > MAX_HUD_CELLS
            || self.cards.iter().filter(|c| c.compass.is_some()).count() > MAX_HUD_COMPASSES
        {
            return Err("HUD has too many cards, rows or cells".into());
        }
        let mut ids = HashSet::new();
        for card in &self.cards {
            if let Some(plate) = &card.text_plate {
                plate.validate(card)?;
            }
            if let Some(compass) = &card.compass {
                compass.validate()?;
                if card.source.is_some()
                    || !card.rows.is_empty()
                    || !card.cells.is_empty()
                    || !(COMPASS_MIN_WIDTH..=COMPASS_MAX_WIDTH).contains(&card.width)
                {
                    return Err("native compass requires its own bounded, empty card".into());
                }
            }
            if let Some(source) = &card.source {
                source.validate()?;
                if !card.cells.is_empty() || card.rows.len() > usize::from(source.limit) {
                    return Err("native HUD source exceeds its reserved rows or has cells".into());
                }
            }
            if !valid_card_id(&card.id) || !ids.insert(&card.id) {
                return Err("HUD cards require bounded unique identifiers".into());
            }
            text(&card.title)?;
            text(&card.editor_label)?;
            if card.reset_scale.is_some_and(|scale| {
                !scale.is_finite() || !(MIN_CARD_SCALE..=MAX_CARD_SCALE).contains(&scale)
            }) || !card.scale.is_finite()
                || !(MIN_CARD_SCALE..=MAX_CARD_SCALE).contains(&card.scale)
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
                || !(if card.cells.is_empty() { 48. } else { 1. }..=MAX_CARD_WIDTH)
                    .contains(&card.width)
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
                if row.item.as_ref().is_some_and(|id| !item_identifier(id)) {
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
#[path = "mod_hud/tests.rs"]
mod test_suite;
