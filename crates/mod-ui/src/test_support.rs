//! Views of retained surface state and pure layout helpers for host-level tests.

use std::sync::Arc;

use json_ui::{Catalog, ViewState};
use serde_json::Value;
use ui::{
    mod_hud::{Card, Hud},
    mod_panel::{Control, Panel},
};
use ui_engine::frame::EngineFrame;

/// The retained personal HUD cards.
pub struct ModWidgetsState<'a> {
    pub catalog: &'a Arc<Catalog>,
    pub content: &'a Hud,
    pub viewport: [f64; 2],
}

/// The HUD editor's draft, pointer state and last drawn frame.
pub struct HudEditorState<'a> {
    pub draft: &'a Hud,
    pub catalog: Option<&'a Arc<Catalog>>,
    pub frame: Option<&'a EngineFrame>,
    pub view: &'a ViewState,
    pub viewport: [f64; 2],
    pub snap: bool,
    /// The captured drag's alignment guides per axis.
    pub drag: Option<[Option<f64>; 2]>,
}

/// The personal panel's catalog, paging and last drawn frame.
pub struct ModPanelState<'a> {
    pub catalog: Option<&'a Arc<Catalog>>,
    pub frame: Option<&'a EngineFrame>,
    pub page: usize,
    pub rows: usize,
    pub category: usize,
    pub pages: usize,
    /// Bind and layout passes its cached screen ran.
    pub passes: usize,
    /// Static root resolutions performed on the renderer's frame path.
    #[cfg(feature = "test-support")]
    pub resolves: usize,
}

/// A card's size in GUI units.
pub fn card_dimensions(card: &Card) -> [f64; 2] {
    crate::mod_widgets::template::dimensions(card)
}

/// A card's top-left in a `viewport` of GUI units.
pub fn card_origin(card: &Card, viewport: [f64; 2]) -> [f64; 2] {
    crate::mod_widgets::template::origin(card, viewport)
}

/// The JSON-UI control authored for `card`, numbering its rows from `row_index`.
pub fn card_control(card: &Card, card_index: usize, row_index: &mut usize) -> Value {
    crate::mod_widgets::template::card(card, card_index, row_index)
}

/// Whether two panels keep one catalog.
pub fn panel_same_shape(a: &Panel, b: &Panel) -> bool {
    crate::mod_panel::same_shape(a, b)
}

/// The colors a panel's theme paints with.
pub fn panel_palette(panel: &Panel) -> crate::mod_panel::Palette {
    crate::mod_panel::Palette::for_panel(panel)
}

/// A dark panel with one control of each kind and no sections.
pub fn sample_panel() -> Panel {
    Panel {
        surface: None,
        reference_size: None,
        theme: Default::default(),
        style: Default::default(),
        title: "Personal controls".into(),
        toggle_key: "ShiftRight".into(),
        dark: true,
        controls: vec![
            Control::Toggle {
                id: "enabled".into(),
                label: "Enabled".into(),
                value: false,
            },
            Control::Slider {
                id: "strength".into(),
                label: "Strength".into(),
                value: 35.0,
                min: 0.0,
                max: 100.0,
                step: 1.0,
            },
            Control::Choice {
                id: "mode".into(),
                label: "Activation".into(),
                index: 0,
                options: vec!["Continuous".into(), "While clicking".into()],
            },
            Control::Button {
                id: "binding".into(),
                label: "Toggle key".into(),
            },
        ],
        sections: Vec::new(),
        capture_key: false,
        motion: false,
    }
}
