//! Gameplay touch controls drawn through JSON-UI; the painted rectangles are the touch regions.

use std::sync::Arc;

use json_ui::{Catalog, Context, DataSource, HitRegion, Scalar, ViewState, hit_test};
use semantic_input::{TouchBounds, TouchContact, TouchControlRegion, touch};
use ui::{FONT_DESIGN_PIXEL_TEXELS, UiNode, UiPoint};
use ui_engine::{
    cached_screen::CachedScreen,
    engine::{ScreenArt, screen::PaintFrame},
    frame::EngineFrame,
    text_metrics::TextMetrics,
};

use super::super::{UiPresentationError, UiPresentationRuntime};
use crate::ui_runtime::UiRuntime;

mod editor;
mod utilities;

const SCREEN: &str = "cinnabar_touch.screen";
const LAYOUT: &[u8] = assets::TOUCH_LAYOUT;
/// Each control's name in the layout, which its `cinnabar.touch.<name>` button id and
/// `#touch_<name>_pressed` binding carry.
const CONTROLS: &[(&str, u16)] = &[
    ("look", touch::LOOK_SURFACE),
    ("joystick", touch::JOYSTICK),
    ("jump", touch::JUMP),
    ("sneak", touch::SNEAK),
    ("sprint", touch::SPRINT),
    ("attack", touch::ATTACK),
    ("use", touch::USE),
    ("menu", touch::MENU),
    ("chat", touch::CHAT),
    ("utilities", touch::UTILITIES),
];
const UTILITY_ACTIONS: &[(&str, u16)] = &[
    ("emote", touch::EMOTE),
    ("debug_overlay", touch::DEBUG_OVERLAY),
    ("player_list", touch::PLAYER_LIST),
    ("dismiss_utilities", touch::DISMISS_UTILITIES),
];

#[derive(Default)]
pub(in super::super) struct TouchControls {
    enabled: bool,
    utilities_open: bool,
    debug_toggle_pending: bool,
    editor: Option<mod_ui::HudEditor>,
    /// Geometry last installed in the editor, independent of ordinary HUD paints.
    editor_viewport: Option<([f32; 2], f32, ui::SafeArea)>,
    pub(super) saved: launcher::menu::settings_options::touch_layout::TouchLayout,
    base: Option<EngineFrame>,
    base_viewport: Option<([f32; 2], f32, ui::SafeArea)>,
    catalog: Option<Arc<Catalog>>,
    screen: CachedScreen,
    /// The overlay and the HUD as the last build painted them, which touches resolve against;
    /// a build that paints neither leaves none.
    frame: Option<EngineFrame>,
    pub(super) hud: Option<EngineFrame>,
    /// Session that produced the retained HUD input geometry.
    pub(super) hud_session: Option<u64>,
    /// Native bottom-HUD artwork bounds from the same completed paint.
    pub(super) hud_keep_out: Option<[f64; 4]>,
    /// The stick knob's drag key and size from the last overlay paint.
    knob: Option<(String, [f64; 2])>,
    /// Held contacts, which press their controls and move the stick's knob.
    contacts: Vec<TouchContact>,
}

impl TouchControls {
    /// Whether gameplay should publish touch-specific HUD controls.
    pub(super) fn enabled(&self) -> bool {
        self.enabled
    }

    /// Retires the last build's geometry, so hidden controls cannot take touches.
    pub(super) fn begin_frame(&mut self) {
        self.frame = None;
        self.hud = None;
        self.hud_session = None;
        self.hud_keep_out = None;
    }
}

impl UiPresentationRuntime {
    /// Shows the gameplay touch controls; platforms without a touchscreen leave them off.
    pub fn enable_touch_controls(&mut self, enabled: bool) {
        self.screens.touch.enabled = enabled;
        if !enabled {
            self.close_touch_utilities();
        }
    }

    pub fn touch_controls_enabled(&self) -> bool {
        self.screens.touch.enabled
    }

    pub fn set_touch_contacts(&mut self, contacts: Vec<TouchContact>) {
        self.screens.touch.contacts = contacts;
    }

    /// The control under a normalized `position` in a `window` of logical pixels: a hotbar slot,
    /// then an overlay control, whose bounds are the painted rectangle.
    pub fn gameplay_touch_region(
        &self,
        position: [f32; 2],
        window: [f32; 2],
    ) -> Option<TouchControlRegion> {
        let state = &self.screens.touch;
        if state.editor.is_some()
            || !state.enabled
            || window.iter().any(|size| !size.is_finite() || *size <= 0.0)
        {
            return None;
        }
        let point = UiPoint::new(position[0] * window[0], position[1] * window[1]).ok()?;
        if state.utilities_open {
            let frame = state.frame.as_ref()?;
            let hit = hit_test(&frame.hits, frame.to_virtual(point))?;
            return region_bounds(frame, hit, window, control_id(hit.pressed.as_deref())?);
        }
        if let Some(frame) = &state.hud
            && let Some(hit) = hit_test(&frame.hits, frame.to_virtual(point))
        {
            let id = if hit.pressed.as_deref() == Some("button.hotbar_inventory_button") {
                Some(touch::INVENTORY)
            } else if hit.collection.as_deref() == Some("hotbar_items") {
                hit.collection_index
                    .filter(|index| *index < 9)
                    .map(|index| touch::HOTBAR_1 + index as u16)
            } else {
                None
            };
            if let Some(id) = id {
                return region_bounds(frame, hit, window, id);
            }
        }
        let frame = state.frame.as_ref()?;
        let hit = hit_test(&frame.hits, frame.to_virtual(point))?;
        let hit_id = control_id(hit.pressed.as_deref())
            .or_else(|| (hit.name == "knob").then_some(touch::JOYSTICK))?;
        // The knob moves, so the stick's activation area is its frame.
        let region = if hit_id == touch::JOYSTICK {
            frame
                .hits
                .iter()
                .find(|region| control_id(region.pressed.as_deref()) == Some(hit_id))?
        } else {
            hit
        };
        region_bounds(frame, region, window, hit_id)
    }

    pub(in super::super) fn append_touch_controls(
        &mut self,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        content: [f32; 2],
    ) -> Result<(), UiPresentationError> {
        let state = &mut self.screens.touch;
        if !state.enabled && state.editor.is_none() {
            return Ok(());
        }
        let Some(renderer) = self.engine.as_deref() else {
            return Ok(());
        };
        let catalog = match &state.catalog {
            Some(catalog) => Arc::clone(catalog),
            None => {
                let catalog = Arc::new(layout_catalog().expect("built-in touch layout parses"));
                state.catalog = Some(Arc::clone(&catalog));
                catalog
            }
        };
        let mut view = ViewState::default();
        if let Some((key, size)) = &state.knob
            && let Some(stick) = state
                .contacts
                .iter()
                .find(|contact| contact.hit_id == Some(touch::JOYSTICK))
        {
            view.drags.insert(
                key.clone(),
                [
                    f64::from(stick.delta[0]) * size[0],
                    -f64::from(stick.delta[1]) * size[1],
                ],
            );
        }
        let mut data = DataSource::new();
        data.set_global("#touch_editor_open", Scalar::Bool(state.editor.is_some()));
        data.set_global(
            "#touch_utilities_open",
            Scalar::Bool(state.utilities_open && state.editor.is_none()),
        );
        data.set_global(
            "#touch_debug_overlay_active",
            Scalar::Bool(self.debug_lines.is_some()),
        );
        data.set_global(
            "#touch_player_list_active",
            Scalar::Bool(runtime.player_list_held()),
        );
        data.set_global("#touch_never_active", Scalar::Bool(false));
        for &(name, id) in CONTROLS {
            let held = state
                .contacts
                .iter()
                .any(|contact| contact.hit_id == Some(id));
            data.set_global(format!("#touch_{name}_pressed"), Scalar::Bool(held));
        }
        let translate = |key: &str| runtime.translation(key);
        let language = runtime.text_generation();
        let mut paint = PaintFrame {
            layouts: &mut self.layouts,
            font: &self.font,
            metrics,
            solid_page: self.solid_texture_page,
            safe_area: self.safe_area,
            content,
            translate: &translate,
            language,
            now: self.menu_seconds,
            nodes,
            next,
            renderers: &mut [],
        };
        let px = metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        let screen = &mut state.screen;
        let viewport_key = (content, px, self.safe_area);
        if state.base_viewport != Some(viewport_key) {
            let rollback = (paint.nodes.len(), *paint.next);
            let base_view = ViewState::default();
            let mut base_data = DataSource::new();
            base_data.set_global("#touch_utilities_open", Scalar::Bool(true));
            state.base = renderer.draw(ScreenArt::default(), &mut paint, |env, root| {
                screen.render_with(
                    SCREEN,
                    &catalog,
                    &Context::default(),
                    base_data,
                    (root, px, language),
                    env,
                    &base_view,
                )
            })?;
            paint.nodes.truncate(rollback.0);
            *paint.next = rollback.1;
            state.base_viewport = Some(viewport_key);
        }
        utilities::place_controls(state, &mut view, content, px);
        if state.utilities_open {
            view.pressed = state.base.as_ref().and_then(|base| {
                base.hits.iter().find_map(|hit| {
                    let id = control_id(hit.pressed.as_deref())?;
                    if !UTILITY_ACTIONS.iter().any(|(_, action)| *action == id) {
                        return None;
                    }
                    state
                        .contacts
                        .iter()
                        .any(|contact| contact.hit_id == Some(id))
                        .then(|| hit.key.clone())
                })
            });
        }
        utilities::place_dropdown(state.base.as_ref(), &mut view, content, px);
        let art = ScreenArt {
            view: Some(&view),
            ..ScreenArt::default()
        };
        state.frame = renderer.draw(art, &mut paint, |env, root| {
            screen.render_with(
                SCREEN,
                &catalog,
                &Context::default(),
                data,
                (root, px, language),
                env,
                &view,
            )
        })?;
        state.knob = state.frame.as_ref().and_then(|frame| {
            let knob = frame.hits.iter().find(|hit| hit.name == "knob")?;
            Some((knob.key.clone(), [knob.rect.w, knob.rect.h]))
        });
        Ok(())
    }
}

fn layout_catalog() -> Result<Catalog, json_ui::LoadError> {
    let mut layout: serde_json::Value =
        serde_json::from_slice(LAYOUT).expect("built-in touch layout parses");
    editor::enable_placement(&mut layout);
    let controls = layout["screen"]["controls"]
        .as_array_mut()
        .expect("touch controls array");
    for (name, text, y, scale) in [
        ("editor_title", "Touch controls", 28., 1.2),
        (
            "editor_help",
            "Drag a control. Guides align edges and centers.",
            44.,
            0.65,
        ),
    ] {
        controls.push(serde_json::json!({name: {"type":"label", "text":text,
            "localize":false, "font_scale_factor":scale, "shadow":true, "text_alignment":"center",
            "anchor_from":"top_middle", "anchor_to":"top_middle", "offset":[0,y],
            "size":["100%",14], "color":[1,1,1], "visible":false,
            "bindings":[{"binding_name":"#touch_editor_open","binding_name_override":"#visible"}]}}));
    }
    let bytes = serde_json::to_vec(&layout).expect("touch layout serializes");
    Catalog::from_files([
        ("ui/_global_variables.json", &b"{}"[..]),
        (
            "ui/_ui_defs.json",
            &br#"{"ui_defs":["ui/cinnabar_touch.json"]}"#[..],
        ),
        ("ui/cinnabar_touch.json", bytes.as_slice()),
    ])
}

fn region_bounds(
    frame: &EngineFrame,
    region: &HitRegion,
    window: [f32; 2],
    hit_id: u16,
) -> Option<TouchControlRegion> {
    let left = region.rect.x.max(region.clip.x);
    let top = region.rect.y.max(region.clip.y);
    let right = (region.rect.x + region.rect.w).min(region.clip.x + region.clip.w);
    let bottom = (region.rect.y + region.rect.h).min(region.clip.y + region.clip.h);
    if left >= right || top >= bottom {
        return None;
    }
    let normalized = |x: f64, y: f64| {
        [
            (frame.origin[0] + x as f32 * frame.scale) / window[0],
            (frame.origin[1] + y as f32 * frame.scale) / window[1],
        ]
    };
    Some(TouchControlRegion {
        hit_id,
        bounds: TouchBounds {
            min: normalized(left, top),
            max: normalized(right, bottom),
        },
    })
}

/// Only permanent controls participate in saved placement and the layout editor.
fn placeable_control(id: u16) -> bool {
    id != touch::LOOK_SURFACE && CONTROLS.iter().any(|&(_, control)| control == id)
}

fn control_id(pressed: Option<&str>) -> Option<u16> {
    let name = pressed?.strip_prefix("cinnabar.touch.")?;
    CONTROLS
        .iter()
        .chain(UTILITY_ACTIONS)
        .find_map(|&(control, id)| (control == name).then_some(id))
}

#[cfg(test)]
mod layout_tests;

#[cfg(test)]
mod editor_tests;

#[cfg(test)]
mod tests {
    use super::*;

    /// Every overlay control names a layout button, so a renamed control cannot go unreachable.
    #[test]
    fn every_control_is_a_layout_button() {
        let mut presentation = crate::test_support::mini_engine_presentation();
        presentation.enable_touch_controls(true);
        presentation.toggle_touch_utilities();
        presentation
            .append_touch_controls(
                &UiRuntime::new(1),
                &mut Vec::new(),
                &mut 0,
                TextMetrics::for_viewport([854, 480], ui::DpiScale::new(1.).unwrap(), None),
                [854., 480.],
            )
            .unwrap();
        let actual: std::collections::BTreeSet<_> = presentation
            .screens
            .touch
            .base
            .as_ref()
            .unwrap()
            .hits
            .iter()
            .filter_map(|hit| hit.pressed.as_deref())
            .filter(|action| action.starts_with("cinnabar.touch."))
            .map(|action| control_id(Some(action)).expect("painted touch action has an input id"))
            .collect();
        let expected: std::collections::BTreeSet<_> = CONTROLS
            .iter()
            .chain(UTILITY_ACTIONS)
            .map(|&(_, id)| id)
            .collect();
        assert_eq!(expected.len(), CONTROLS.len() + UTILITY_ACTIONS.len());
        assert_eq!(actual, expected);
    }

    /// Hotbar cells and overlay controls from an earlier build cannot take touches once a build
    /// stops painting them, as when the HUD is hidden.
    #[test]
    fn a_build_that_paints_nothing_leaves_no_touch_regions() {
        let painted = || EngineFrame {
            hits: Arc::from([]),
            report: json_ui::LayoutReport::default(),
            cancel_target: None,
            origin: [0.0; 2],
            scale: 1.0,
            panel: None,
            edit_texts: Vec::new(),
            top: Vec::new(),
        };
        let mut controls = TouchControls {
            frame: Some(painted()),
            hud: Some(painted()),
            ..TouchControls::default()
        };
        controls.begin_frame();
        assert!(controls.frame.is_none() && controls.hud.is_none());
    }
}
