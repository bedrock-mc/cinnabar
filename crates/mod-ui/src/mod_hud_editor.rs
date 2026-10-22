//! Native layout editing of bounded HUD previews. Pointer data never enters a guest.

mod actions;
mod caption;
mod data;
mod feedback;
mod history;
mod host;
mod input;
mod inspector;
mod motion;
mod paint;
mod publication;
mod slider;
mod snapping;
mod template;

#[cfg(test)]
mod amendment_render_tests;
#[cfg(test)]
mod lifecycle_tests;

use crate::{
    ModUi, RowIcons, mod_panel::transitions::SurfaceTransitions, mod_widgets::template as cards,
};
use json_ui::{Catalog, Context, DataSource, Scalar, ViewState};
use std::sync::Arc;
use ui::{
    FONT_DESIGN_PIXEL_TEXELS, IconRef,
    mod_hud::{EditorResult, Hud},
};
use ui_engine::frame::EngineFrame;
use ui_engine::{
    cached_screen::CachedScreen,
    engine::{
        FormEngine, ScreenArt,
        glass::{GlassRenderer, paint_world_backdrop},
        item_renderer::ItemRenderer,
        screen::{CustomRenderer, PaintFrame},
    },
};

const SCREEN: &str = "cinnabar_hud_editor.layout";
const GUIDE_WIDTH: f64 = 0.75;
/// Distance pill height and horizontal text padding, in GUI pixels.
const MEASUREMENT_PILL_HEIGHT: f64 = 9.;
const MEASUREMENT_PILL_PADDING: f64 = 4.;
/// Distance label font scale (about 3.5 px glyphs) and a digit advance that covers it.
const MEASUREMENT_FONT_SCALE: f64 = 0.45;
const MEASUREMENT_DIGIT_ADVANCE: f64 = 3.;
/// Touch distance labels remain readable independently of gameplay geometry.
const TOUCH_MEASUREMENT_SCALE: f64 = 1.6;
const AXIS_SNAP_DISTANCE: f64 = 6.;
const GEOMETRY_EPSILON: f64 = 0.001;
const EDITOR_MARGIN: f64 = 8.;

/// A host-owned layout transaction sharing preview geometry, snapping, history and editor chrome.
pub struct HudEditor {
    draft: Hud,
    committed: Hud,
    hidden: std::collections::HashSet<String>,
    committed_hidden: std::collections::HashSet<String>,
    history: history::History,
    toast: motion::Toast,
    anchor_motion: Option<motion::AnchorMotion>,
    pub(super) autosave: bool,
    pub(super) close_requested: bool,
    /// Reset intent for the next result; autosave clears it after publication.
    reset: bool,
    history_buttons: bool,
    chrome_scale: f64,
    grid: bool,
    snap: bool,
    bypass_snap: bool,
    data_dirty: bool,
    snap_dirty: bool,
    row_icons: Vec<Option<IconRef>>,
    icons: Vec<IconRef>,
    keep_out: Vec<[f64; 4]>,
    selected: Option<usize>,
    /// Card whose bounds or resize handle was under the pointer at its last event.
    hovered: Option<usize>,
    pub(super) drag: Option<input::Drag>,
    slider: Option<slider::Gesture>,
    /// `(control index, fill)` for every guest slider, as last published.
    control_fills: Vec<(usize, f64)>,
    pub(super) open: bool,
    result: Option<EditorResult>,
    viewport: [f64; 2],
    inspector: inspector::Placement,
    caption_width: f64,
    caption_scale_width: f64,
    caption_bounds: Option<[f64; 4]>,
    catalog: Option<Arc<Catalog>>,
    screen: CachedScreen,
    data: Arc<DataSource>,
    frame: Option<EngineFrame>,
    view: ViewState,
    transitions: SurfaceTransitions,
}

impl ModUi {
    /// Opens bounded previews independently of whether their gameplay cards are enabled.
    pub fn open_mod_hud_editor(&mut self, preview: &Hud) -> Result<(), String> {
        preview.validate()?;
        if !self.mod_panel_open() {
            return Err("HUD editor requires an open panel".into());
        }
        let transitions = self.mod_hud_editor.take().and_then(|mut previous| {
            (!previous.open
                && previous.transitions.exiting()
                && previous.result.is_none()
                && previous.draft.surface == preview.surface)
                .then(|| {
                    previous.transitions.cancel_exit();
                    previous.transitions
                })
        });
        self.cancel_mod_panel_edit();
        let mut editor = HudEditor::new(preview)?;
        editor.transitions = transitions.unwrap_or_default();
        editor.snap = false;
        editor.history_buttons = false;
        self.mod_hud_editor = Some(editor);
        Ok(())
    }

    /// Supplies safe-content GUI-pixel rectangles from the host's native HUD layout.
    /// Each rectangle is `[x, y, width, height]`; it guides placement without prohibiting overlap.
    pub fn set_mod_hud_editor_keep_out(&mut self, rects: &[[f64; 4]]) {
        let Some(editor) = self.mod_hud_editor.as_mut().filter(|e| e.open) else {
            return;
        };
        if editor.keep_out == rects {
            return;
        }
        editor.snap_dirty = true;
        editor.keep_out.clear();
        editor.keep_out.extend(
            rects
                .iter()
                .copied()
                .filter(|r| r.iter().all(|v| v.is_finite()) && r[2] > 0. && r[3] > 0.),
        );
    }

    /// Temporarily bypasses every placement snap while the host observes Shift held.
    pub fn set_mod_hud_editor_shift(&mut self, held: bool) {
        if let Some(editor) = self.mod_hud_editor.as_mut().filter(|e| e.open) {
            editor.set_snap_bypass(held);
        }
    }

    /// Whether native layout editing currently owns the personal panel surface.
    pub fn mod_hud_editor_open(&self) -> bool {
        self.mod_hud_editor
            .as_ref()
            .is_some_and(|editor| editor.open)
    }

    /// Whether editor previews still draw, including a closed surface playing its exit.
    pub fn mod_hud_editor_painting(&self) -> bool {
        self.mod_hud_editor
            .as_ref()
            .is_some_and(HudEditor::painting)
    }

    /// Finishes closing motion before the caller chooses between previews and the live HUD.
    pub fn advance_mod_hud_editor_exit(&mut self, now: f64) {
        if let Some(editor) = self.mod_hud_editor.as_mut().filter(|editor| !editor.open) {
            editor.advance_transitions(now, !self.reduced_motion);
        }
    }

    /// Whether editor surface bindings need another frame to finish opening or closing.
    pub(super) fn mod_hud_editor_motion_active(&self) -> bool {
        self.mod_hud_editor.as_ref().is_some_and(|editor| {
            editor.transitions.exiting()
                || editor.open
                    && (editor.transitions.active()
                        || editor.anchor_motion.is_some()
                        || !editor.toast.text.is_empty())
        })
    }

    /// Cancels a draft on ownership, focus, session or presentation loss.
    pub fn cancel_mod_hud_editor(&mut self) {
        if let Some(editor) = self.mod_hud_editor.as_mut() {
            if editor.open {
                editor.finish(false);
            }
            editor.transitions.cancel_exit();
            editor.frame = None;
        }
    }

    /// Host-owned result, delivered only to the component that requested this draft.
    pub fn take_mod_hud_editor_result(&mut self) -> Option<EditorResult> {
        let editor = self.mod_hud_editor.as_mut()?;
        let result = editor.result.take()?;
        if !editor.painting() {
            self.mod_hud_editor = None;
        }
        Some(result)
    }

    /// Each painted draft row's item icon, including while a closed surface plays its exit.
    pub fn mod_hud_editor_icons(
        &self,
        icon: impl Fn(&str, u32) -> Option<IconRef>,
    ) -> Option<RowIcons> {
        let editor = self.mod_hud_editor.as_ref().filter(|e| e.painting())?;
        Some(RowIcons::resolve(&editor.draft, icon))
    }

    /// Draws previews and editor chrome at the same GUI scale as gameplay cards; `resolved`
    /// holds [`Self::mod_hud_editor_icons`].
    pub fn paint_mod_hud_editor(
        &mut self,
        engine: Option<&FormEngine>,
        paint_frame: &mut PaintFrame<'_>,
        resolved: &RowIcons,
    ) {
        let Some(editor) = self.mod_hud_editor.as_mut() else {
            return;
        };
        let controls = self
            .mod_panel
            .as_ref()
            .map_or(&[][..], crate::mod_panel::ModPanel::controls);
        editor.paint_with(
            engine,
            paint_frame,
            resolved,
            !self.reduced_motion,
            controls,
            |data| self.local_player_identity.write(data),
        );
        if !editor.painting() {
            self.set_mod_panel_open(false);
        }
    }
}

impl HudEditor {
    /// Removes retained closing visuals immediately when presentation ownership is lost.
    pub(super) fn stop_exit(&mut self) {
        self.transitions.cancel_exit();
        self.frame = None;
    }

    /// Separates logical input ownership from the lifetime of an animated closing surface.
    fn painting(&self) -> bool {
        self.open || self.transitions.exiting()
    }

    /// Samples the caller's presentation clock and releases a completed or disabled exit.
    fn advance_transitions(&mut self, now: f64, animate: bool) {
        if let Some(surface) = &self.draft.surface {
            self.data_dirty |= self.transitions.advance(surface, now, animate);
        }
        if !self.open && (!animate || !self.transitions.active()) {
            self.stop_exit();
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl ModUi {
    pub fn mod_hud_editor_state(&self) -> Option<crate::test_support::HudEditorState<'_>> {
        let editor = self.mod_hud_editor.as_ref()?;
        Some(crate::test_support::HudEditorState {
            draft: &editor.draft,
            nodes: editor.screen.nodes(),
            catalog: editor.catalog.as_ref(),
            frame: editor.frame.as_ref(),
            view: &editor.view,
            viewport: editor.viewport,
            grid: editor.grid,
            snap: editor.snap,
            passes: editor.screen.passes,
            drag: editor.drag.as_ref().map(|drag| drag.guides),
        })
    }
}
