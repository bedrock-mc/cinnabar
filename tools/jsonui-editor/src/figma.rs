//! Figma frames to JSON-UI. The Figma plugin serializes the selected layer tree
//! as [`Node`] JSON and [`convert`] maps it deterministically to a pack's UI
//! file, `_ui_defs.json` and textures, warning about every layer or property it
//! cannot express. The conversion loads into a [`Session`] like any other pack.

mod convert;
mod names;
mod place;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::diagnose::Diagnostic;
use crate::mock::MockData;
use crate::scene::{Session, View};

pub use convert::convert;

/// The layer-name tokens the plugin acts on: `raster` layers arrive as a PNG
/// export ([`Node::raster`]), `button` instances with their [`Node::variants`].
pub fn plugin_directives() -> serde_json::Value {
    serde_json::json!({ "raster": names::IMAGE, "button": names::BUTTON })
}

/// One serialized layer. Positions are relative to the parent layer's top left.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Node {
    pub id: String,
    pub name: String,
    /// Figma's node type (`FRAME`, `TEXT`, `RECTANGLE`, ...).
    #[serde(rename = "type")]
    pub kind: String,
    pub visible: bool,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// Degrees.
    pub rotation: f64,
    pub opacity: f64,
    pub blend_mode: String,
    pub is_mask: bool,
    pub constraints: Constraints,
    /// Bottom to top, as Figma lists them.
    pub fills: Vec<Paint>,
    pub strokes: Vec<Paint>,
    pub effects: Vec<Effect>,
    pub corner_radius: f64,
    pub clips_content: bool,
    /// Present on auto-layout frames.
    pub layout: Option<AutoLayout>,
    /// How this layer sits in an auto-layout parent.
    pub positioning: Positioning,
    /// Horizontal and vertical sizing in an auto-layout parent.
    pub sizing: [Sizing; 2],
    /// Figma's `minWidth`/`minHeight`, where set.
    pub min_size: [Option<f64>; 2],
    /// Figma's `maxWidth`/`maxHeight`, where set.
    pub max_size: [Option<f64>; 2],
    pub text: Option<Text>,
    /// Image key of the plugin's PNG export of this layer (`@image`).
    pub raster: Option<String>,
    /// The layer draws outside its box, which its export cuts off.
    pub overflows: bool,
    pub children: Vec<Node>,
    /// A variant component's or instance's variant property values.
    pub variant_properties: BTreeMap<String, String>,
    /// A `@button` instance swapped to each variant of its component set.
    pub variants: Vec<Node>,
}

impl Default for Node {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            kind: String::new(),
            visible: true,
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
            rotation: 0.0,
            opacity: 1.0,
            blend_mode: String::new(),
            is_mask: false,
            constraints: Constraints::default(),
            fills: Vec::new(),
            strokes: Vec::new(),
            effects: Vec::new(),
            corner_radius: 0.0,
            clips_content: false,
            layout: None,
            positioning: Positioning::Auto,
            sizing: [Sizing::Fixed; 2],
            min_size: [None; 2],
            max_size: [None; 2],
            text: None,
            raster: None,
            overflows: false,
            children: Vec::new(),
            variant_properties: BTreeMap::new(),
            variants: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Constraints {
    pub horizontal: Constraint,
    pub vertical: Constraint,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Constraint {
    #[default]
    Min,
    Max,
    Center,
    Stretch,
    Scale,
    #[serde(other)]
    Other,
}

/// A fill or stroke.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Paint {
    /// `SOLID`, `IMAGE`, `GRADIENT_LINEAR`, ...
    #[serde(rename = "type")]
    pub kind: String,
    pub visible: bool,
    pub opacity: f64,
    /// Linear `[r, g, b]` in 0..1 for `SOLID`.
    pub color: Option<[f64; 3]>,
    /// Image key for `IMAGE`.
    pub image: Option<String>,
    /// `FILL`, `FIT`, `CROP` or `TILE` for `IMAGE`.
    pub scale_mode: String,
    /// Exposure, contrast or other image adjustments are set.
    pub has_filters: bool,
    /// The paint's own blend mode.
    pub blend_mode: String,
    /// An image paint's own rotation, in degrees.
    pub rotation: f64,
}

impl Default for Paint {
    fn default() -> Self {
        Self {
            kind: String::new(),
            visible: true,
            opacity: 1.0,
            color: None,
            image: None,
            scale_mode: String::new(),
            has_filters: false,
            blend_mode: String::new(),
            rotation: 0.0,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Effect {
    #[serde(rename = "type")]
    pub kind: String,
    pub visible: bool,
}

impl Default for Effect {
    fn default() -> Self {
        Self {
            kind: String::new(),
            visible: true,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AutoLayout {
    pub mode: LayoutMode,
    pub item_spacing: f64,
    /// `[left, top, right, bottom]`.
    pub padding: [f64; 4],
    pub primary_align: Align,
    pub counter_align: Align,
    pub wrap: bool,
    /// Figma's `itemReverseZIndex`: the first layer draws on top.
    pub reverse_z_index: bool,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LayoutMode {
    Horizontal,
    #[default]
    Vertical,
    #[serde(other)]
    Other,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Align {
    #[default]
    Min,
    Center,
    Max,
    SpaceBetween,
    Baseline,
    #[serde(other)]
    Other,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Positioning {
    #[default]
    Auto,
    Absolute,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Sizing {
    #[default]
    Fixed,
    Hug,
    Fill,
}

/// A text layer's content and style. `mixed` names properties that vary
/// across the text, whose first run's value the other fields hold.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Text {
    pub characters: String,
    pub font_family: String,
    pub font_style: String,
    pub font_size: f64,
    /// `LEFT`, `CENTER`, `RIGHT` or `JUSTIFIED`.
    pub align_h: String,
    /// `TOP`, `CENTER` or `BOTTOM`.
    pub align_v: String,
    /// `WIDTH_AND_HEIGHT`, `HEIGHT`, `NONE` or `TRUNCATE`.
    pub auto_resize: String,
    /// Pixels.
    pub letter_spacing: f64,
    /// Pixels; `None` is Figma's automatic line height.
    pub line_height: Option<f64>,
    pub text_case: String,
    pub text_decoration: String,
    pub truncate: bool,
    /// Some of the text uses list formatting.
    pub list: bool,
    pub mixed: Vec<String>,
}

impl Default for Text {
    fn default() -> Self {
        Self {
            characters: String::new(),
            font_family: String::new(),
            font_style: "Regular".into(),
            font_size: 8.0,
            align_h: "LEFT".into(),
            align_v: "TOP".into(),
            auto_resize: "WIDTH_AND_HEIGHT".into(),
            letter_spacing: 0.0,
            line_height: None,
            text_case: "ORIGINAL".into(),
            text_decoration: "NONE".into(),
            truncate: false,
            list: false,
            mixed: Vec::new(),
        }
    }
}

/// Something the conversion could not express exactly.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Warning {
    pub node_id: String,
    /// Layer names from the converted frame down, joined by ` / `.
    pub layer: String,
    pub message: String,
}

/// A converted frame: the pack's files and what the conversion left out.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Conversion {
    pub namespace: String,
    /// `namespace.name` of the generated screen.
    pub screen: String,
    /// The frame's size in virtual pixels.
    pub size: [f64; 2],
    /// Pack-relative path to contents.
    #[serde(skip)]
    pub files: BTreeMap<String, Vec<u8>>,
    pub warnings: Vec<Warning>,
    /// Named label fonts (`font_type`s) the preview draws with its one font.
    pub font_types: Vec<String>,
    /// Bound values as the design shows them, so a preview matches the frame.
    #[serde(skip)]
    pub mock: MockData,
}

impl Conversion {
    pub fn ui_path(&self) -> String {
        format!("ui/{}.json", self.namespace)
    }

    /// Replaces `session`'s packs with this one (keeping its font) and returns
    /// the screen's view at the frame's size, at the largest GUI scale up to
    /// `gui_scale` whose window still lays the frame out 1:1.
    pub fn load(&self, session: &mut Session, gui_scale: u8) -> View {
        let mut fresh = Session::default();
        std::mem::swap(&mut fresh.fonts, &mut session.fonts);
        *session = fresh;
        let layer = session.workspace.add_layer(&self.namespace);
        let files = self
            .files
            .iter()
            .map(|(path, bytes)| (path.clone(), bytes.clone()))
            .collect();
        session.workspace.add_files(layer, files, Vec::new());
        let view = |scale: u8| View {
            reference: self.screen.clone(),
            size: self.size.map(|extent| {
                extent.ceil().clamp(1.0, 8192.0 / f64::from(scale)) as u32 * u32::from(scale)
            }),
            gui_scale: Some(scale),
            context: BTreeMap::new(),
            mock: self.mock.clone(),
        };
        // Windows below Bedrock's minimum GUI viewport clamp the scale down.
        (1..=gui_scale.max(1))
            .rev()
            .map(view)
            .find(|view| Some(view.px()) == view.gui_scale.map(u32::from))
            .unwrap_or_else(|| view(1))
    }

    /// Diagnostics for the generated UI file once [`Self::load`]ed into `session`.
    pub fn validate(&self, session: &mut Session) -> Vec<Diagnostic> {
        crate::api::validate(session, &[self.ui_path()], &json_ui::Context::empty())
    }
}
