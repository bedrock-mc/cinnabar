//! Native layout editing of bounded HUD previews. Pointer data never enters a guest.

mod actions;
mod data;
mod feedback;
mod history;
mod input;
mod motion;
mod publication;
mod snapping;
mod template;

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
const MEASUREMENT_PILL: [f64; 2] = [36., 12.];
const AXIS_SNAP_DISTANCE: f64 = 6.;

pub(super) struct HudEditor {
    draft: Hud,
    committed: Hud,
    hidden: std::collections::HashSet<String>,
    committed_hidden: std::collections::HashSet<String>,
    history: history::History,
    toast: motion::Toast,
    anchor_motion: Option<motion::AnchorMotion>,
    pub(super) autosave: bool,
    pub(super) close_requested: bool,
    reset: bool,
    snap: bool,
    bypass_snap: bool,
    data_dirty: bool,
    snap_dirty: bool,
    row_icons: Vec<Option<IconRef>>,
    icons: Vec<IconRef>,
    keep_out: Vec<[f64; 4]>,
    selected: Option<usize>,
    pub(super) drag: Option<input::Drag>,
    pub(super) open: bool,
    result: Option<EditorResult>,
    viewport: [f64; 2],
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
        let mut preview = preview.clone();
        for card in &mut preview.cards {
            card.reset_anchor.get_or_insert(card.anchor);
            card.reset_offset.get_or_insert(card.offset);
            card.reset_scale.get_or_insert(card.scale);
        }
        self.mod_hud_editor = Some(HudEditor {
            autosave: preview.autosave,
            draft: preview.clone(),
            history: history::History::new(&preview),
            hidden: Default::default(),
            committed_hidden: Default::default(),
            toast: Default::default(),
            anchor_motion: None,
            committed: preview,
            close_requested: false,
            reset: false,
            snap: false,
            bypass_snap: false,
            data_dirty: true,
            snap_dirty: false,
            row_icons: Vec::new(),
            icons: Vec::new(),
            keep_out: Vec::new(),
            selected: None,
            drag: None,
            open: true,
            result: None,
            viewport: [0.; 2],
            catalog: None,
            screen: CachedScreen::default(),
            data: Arc::new(DataSource::new()),
            frame: None,
            view: ViewState::default(),
            transitions: transitions.unwrap_or_default(),
        });
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
            editor.snap_dirty |= editor.bypass_snap != held;
            editor.data_dirty |= editor.bypass_snap != held;
            editor.bypass_snap = held;
            if held && let Some(drag) = &mut editor.drag {
                drag.guides = [None; 2];
                drag.targets = [None; 2];
            }
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
        let Some(renderer) = engine else {
            self.cancel_mod_hud_editor();
            self.set_mod_panel_open(false);
            return;
        };
        let Some(editor) = self.mod_hud_editor.as_mut().filter(|e| e.painting()) else {
            return;
        };
        let px = paint_frame.metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        let content = paint_frame.content;
        let viewport = [f64::from(content[0] / px), f64::from(content[1] / px)];
        if viewport[0] < 220. || viewport[1] < 120. {
            editor.finish(false);
            self.set_mod_panel_open(false);
            return;
        }
        if editor.viewport != viewport {
            editor.data_dirty = true;
            editor.viewport = viewport;
            editor.catalog = None;
            editor.frame = None;
            editor.cancel_pointer_input();
            editor.anchor_motion = None;
        }
        editor.advance_transitions(paint_frame.now, !self.reduced_motion);
        editor.advance_feedback(paint_frame.now, !self.reduced_motion);
        if !editor.painting() {
            return;
        }
        let icons_changed = editor.row_icons != resolved.as_slice();
        if editor.data_dirty || icons_changed {
            let mut data = editor.data();
            if editor.draft.surface.is_some() {
                editor.transitions.write(&mut data);
            }
            editor.row_icons.clear();
            editor.row_icons.extend_from_slice(resolved.as_slice());
            editor.icons.clear();
            for (index, icon) in resolved.as_slice().iter().enumerate() {
                let scalar = if let Some(icon) = icon {
                    let id = editor.icons.len();
                    editor.icons.push(*icon);
                    Scalar::Num(id as f64)
                } else {
                    Scalar::Json(serde_json::Value::Null)
                };
                data.set_global(format!("#row_{index}_icon"), scalar);
            }
            if *editor.data != data {
                editor.data = Arc::new(data);
            }
            editor.data_dirty = false;
        }
        self.local_player_identity.write(&mut editor.data);
        let rollback = (paint_frame.nodes.len(), *paint_frame.next);
        let language = paint_frame.language;
        let world = editor
            .draft
            .surface
            .as_ref()
            .map(|surface| editor.transitions.world_backdrop(surface))
            .unwrap_or_default();
        if let Err(error) = paint_world_backdrop(paint_frame, world) {
            paint_frame.nodes.truncate(rollback.0);
            *paint_frame.next = rollback.1;
            editor.finish(false);
            tracing::warn!(%error, "HUD editor disabled after rendering failure");
            self.set_mod_panel_open(false);
            return;
        }
        let mut items = ItemRenderer {
            icons: &editor.icons,
            id_aux: &[],
        };
        let mut glass = GlassRenderer::new(world);
        let mut zones = feedback::KeepOutRenderer {
            rects: &editor.keep_out,
            dragging: editor.drag.is_some(),
        };
        let mut renderers: [&mut dyn CustomRenderer; 3] = [&mut items, &mut glass, &mut zones];
        // Keep pointer feedback outside the editor borrow while measuring native hit bounds.
        let view = std::mem::take(&mut editor.view);
        let art = ScreenArt {
            view: Some(&view),
            ..Default::default()
        };
        let result = renderer.draw(
            art,
            &mut paint_frame.with_renderers(&mut renderers),
            |env, root| {
                if crate::mod_widgets::text_plate::measure(&mut editor.draft, env.text) {
                    editor.catalog = None;
                    let data = Arc::make_mut(&mut editor.data);
                    for (n, card) in editor.draft.cards.iter().enumerate() {
                        crate::mod_widgets::text_plate::data(data, card, n, viewport);
                    }
                    data::write_bounds(data, &editor.draft, viewport, editor.selected);
                    data::write_selection(data, &editor.draft, viewport, editor.selected);
                    feedback::measurements(data, &editor.draft, viewport, editor.selected);
                }
                if editor.catalog.is_none() {
                    let catalog = match template::catalog(&editor.draft, viewport) {
                        Ok(catalog) => catalog,
                        Err(error) => {
                            tracing::warn!(%error,"HUD editor template rejected");
                            return None;
                        }
                    };
                    editor.catalog = Some(Arc::new(catalog));
                    editor.screen = CachedScreen::default();
                }
                editor.screen.render_shared_with(
                    SCREEN,
                    editor.catalog.as_ref().expect("installed"),
                    &Context::default(),
                    Arc::clone(&editor.data),
                    (root, px, language),
                    env,
                    &view,
                )
            },
        );
        editor.view = view;
        match result {
            Ok(Some(frame)) => editor.frame = Some(frame),
            result => {
                paint_frame.nodes.truncate(rollback.0);
                *paint_frame.next = rollback.1;
                editor.finish(false);
                if let Err(error) = result {
                    tracing::warn!(%error, "HUD editor disabled after rendering failure");
                }
                self.set_mod_panel_open(false);
            }
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
            snap: editor.snap,
            passes: editor.screen.passes,
            drag: editor.drag.as_ref().map(|drag| drag.guides),
        })
    }
}
