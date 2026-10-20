//! Native layout editing of bounded HUD previews. Pointer data never enters a guest.

mod data;
mod input;
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
const AXIS_SNAP_DISTANCE: f64 = 6.;

pub(super) struct HudEditor {
    draft: Hud,
    committed: Hud,
    pub(super) autosave: bool,
    pub(super) close_requested: bool,
    reset: bool,
    snap: bool,
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
        }
        self.mod_hud_editor = Some(HudEditor {
            autosave: preview.autosave,
            draft: preview.clone(),
            committed: preview,
            close_requested: false,
            reset: false,
            snap: false,
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
            editor.transitions.exiting() || editor.open && editor.transitions.active()
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
            editor.viewport = viewport;
            editor.catalog = None;
            editor.frame = None;
            editor.cancel_pointer_input();
        }
        editor.advance_transitions(paint_frame.now, !self.reduced_motion);
        if !editor.painting() {
            return;
        }
        let mut data = editor.data();
        if editor.draft.surface.is_some() {
            editor.transitions.write(&mut data);
        }
        let mut icons = Vec::<IconRef>::new();
        for (index, icon) in resolved.as_slice().iter().enumerate() {
            let scalar = if let Some(icon) = icon {
                let id = icons.len();
                icons.push(*icon);
                Scalar::Num(id as f64)
            } else {
                Scalar::Json(serde_json::Value::Null)
            };
            data.set_global(format!("#row_{index}_icon"), scalar);
        }
        if *editor.data != data {
            editor.data = Arc::new(data);
        }
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
            icons: &icons,
            id_aux: &[],
        };
        let mut glass = GlassRenderer::new(world);
        let mut renderers: [&mut dyn CustomRenderer; 2] = [&mut items, &mut glass];
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
                }
                if editor.catalog.is_none() {
                    let catalog = match template::catalog(editor, viewport) {
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
            self.transitions.advance(surface, now, animate);
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
