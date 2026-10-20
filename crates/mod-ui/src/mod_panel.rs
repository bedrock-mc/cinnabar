//! A private JSON-UI catalog for bounded personal-extension controls.

mod compact;
mod data;
mod edit;
mod field;
mod field_paint;
mod hud_cards;
mod icons;
mod input;
mod layout;
mod motion;
pub(crate) mod prepared;
mod scrolling;
pub(crate) mod surface;
mod template;
pub(crate) mod transitions;
mod vector;
mod widgets;

#[cfg(any(test, feature = "test-support"))]
pub(crate) use template::same_shape;

#[cfg(test)]
use json_ui::Context;
use std::sync::Arc;

use json_ui::{Catalog, DataSource, ViewState};
use ui::{
    FONT_DESIGN_PIXEL_TEXELS, UiScale,
    mod_panel::{Control, Event, Panel},
};

use crate::{ModUi, PanelUiOwner};
use data::control_data;
use ui_engine::frame::EngineFrame;
use ui_engine::{
    cached_screen::CachedScreen,
    engine::{
        FormEngine, ScreenArt,
        glass::{GlassRenderer, paint_world_backdrop},
        screen::{CustomRenderer, PaintFrame},
    },
};
pub(crate) use widgets::Palette;

const SCREEN: &str = "cinnabar_personal.panel";
const ROW_HEIGHT: f64 = 26.0;
const CHROME_HEIGHT: f64 = 88.0;

pub(super) struct ModPanel {
    owner: PanelUiOwner,
    hud_previews: hud_cards::Previews,
    panel: Panel,
    open: bool,
    catalog: Option<Arc<Catalog>>,
    screen: CachedScreen,
    frame: Option<EngineFrame>,
    viewport: [f64; 2],
    page: usize,
    category: usize,
    pages: usize,
    rows: usize,
    drag: Option<usize>,
    vector_drag: Option<vector::Drag>,
    scroll_drag: Option<(String, f64)>,
    view: ViewState,
    data: Arc<DataSource>,
    pointer: Option<[f32; 2]>,
    held: bool,
    edit: Option<edit::Editor>,
    /// Native feedback and surface transitions, including an exit after closing.
    motion: motion::PanelMotion,
    motion_data_dirty: bool,
    text_events: Vec<ui::mod_panel::TextEvent>,
    field_events: Vec<ui::mod_panel::FieldEvent>,
    prepared: Option<Arc<prepared::PreparedRoute>>,
}

impl ModUi {
    /// Reports distinct unknown HUD selectors for rendering regressions.
    #[cfg(test)]
    pub(crate) fn hud_preview_unknowns(&self) -> usize {
        self.mod_panel
            .as_ref()
            .map_or(0, |panel| panel.hud_previews.unknown_ids)
    }

    /// Preview declarations of the active or exiting panel; other owners remain inaccessible.
    pub fn control_previews(
        &self,
    ) -> Option<(
        PanelUiOwner,
        &std::collections::BTreeMap<String, ui::mod_panel::preview::ControlPreview>,
    )> {
        if self.mod_hud_editor_painting() {
            return None;
        }
        let panel = self
            .mod_panel
            .as_ref()
            .filter(|panel| panel.open || panel.exiting())?;
        Some((panel.owner, &panel.panel.surface.as_ref()?.previews))
    }

    /// Drains committed native text edits once for the active personal-panel owner.
    pub fn take_mod_panel_text_events(
        &mut self,
        owner: PanelUiOwner,
    ) -> Vec<ui::mod_panel::TextEvent> {
        self.mod_panel
            .as_mut()
            .filter(|panel| panel.owner == owner)
            .map_or_else(Vec::new, |panel| std::mem::take(&mut panel.text_events))
    }
    /// Drains live edits, submissions and navigation once for the matching panel owner.
    pub fn take_mod_panel_field_events(
        &mut self,
        owner: PanelUiOwner,
    ) -> Vec<ui::mod_panel::FieldEvent> {
        self.mod_panel
            .as_mut()
            .filter(|panel| panel.owner == owner)
            .map_or_else(Vec::new, |panel| panel.drain_field_events())
    }

    /// Drops the panel's laid-out text and pointer capture after its private font changed.
    pub fn invalidate_mod_panel_font(&mut self) {
        self.cancel_mod_hud_editor();
        for owner in self.prepared_panel_ui.values_mut() {
            for screen in &mut owner.screens {
                *screen = None;
            }
        }
        if let Some(panel) = &mut self.mod_panel {
            panel.cancel_edit();
            panel.screen = CachedScreen::default();
            panel.frame = None;
            panel.drag = None;
            panel.vector_drag = None;
            panel.scroll_drag = None;
            panel.pointer = None;
        }
    }

    /// Releases all panel catalogs and input when the last local instance leaves.
    pub fn clear_mod_panels(&mut self) {
        self.cancel_mod_hud_editor();
        self.mod_panel = None;
        self.prepared_panel_ui.clear();
    }

    /// Publishes one owner's panel, retaining its open state across value updates.
    /// Passing no panel clears the selection only when it belongs to this owner.
    pub fn set_mod_panel(
        &mut self,
        owner: PanelUiOwner,
        panel: Option<&Panel>,
    ) -> Result<(), String> {
        let Some(panel) = panel else {
            if self
                .mod_panel
                .as_ref()
                .is_some_and(|panel| panel.owner == owner)
            {
                self.cancel_mod_hud_editor();
                self.mod_panel = None;
            }
            return Ok(());
        };
        if self
            .mod_panel
            .as_ref()
            .is_some_and(|current| current.owner == owner && current.panel == *panel)
        {
            return Ok(());
        }
        let same_shape = self.mod_panel.as_ref().is_some_and(|current| {
            current.owner == owner && template::same_shape(&current.panel, panel)
        });
        let prepared = self
            .prepared_panel_ui
            .get(&owner)
            .and_then(|prepared| prepared.prepared.route(panel));
        if same_shape || prepared.is_some() {
            panel.validate_values()?;
        } else {
            panel.validate()?;
        }
        if !same_shape {
            self.cancel_mod_hud_editor();
        }
        if let Some(current) = self.mod_panel.as_mut() {
            if current.owner != owner {
                current.hud_previews = hud_cards::Previews::default();
            }
            if !same_shape {
                let transitions = (current.owner == owner
                    && current.open
                    && current.panel.surface.is_some()
                    && panel.surface.is_some())
                .then(|| std::mem::take(&mut current.motion.transitions));
                current.text_events.clear();
                current.field_events.clear();
                current.owner = owner;
                current.edit = None;
                current.screen = CachedScreen::default();
                current.catalog = None;
                current.frame = None;
                current.drag = None;
                current.vector_drag = None;
                current.scroll_drag = None;
                if current.panel.sections != panel.sections
                    || current.panel.controls.len() != panel.controls.len()
                {
                    current.page = 0;
                    current.category = 0;
                }
                current.view = ViewState::default();
                current.motion = motion::PanelMotion::default();
                if let Some(transitions) = transitions {
                    current.motion.transitions = transitions;
                }
                current.pointer = None;
                current.prepared = prepared.clone();
                current.catalog = prepared.as_ref().map(|route| Arc::clone(&route.catalog));
            }
            if panel.capture_key && !current.editing_text() {
                current.cancel_edit();
            }
            current.reconcile_editor_value(panel);
            current.panel = panel.clone();
            current.refresh_data();
        } else {
            self.mod_panel = Some(ModPanel {
                owner,
                hud_previews: hud_cards::Previews::default(),
                panel: panel.clone(),
                open: false,
                catalog: prepared.as_ref().map(|route| Arc::clone(&route.catalog)),
                screen: CachedScreen::default(),
                frame: None,
                viewport: [0.0; 2],
                page: 0,
                category: 0,
                pages: 1,
                rows: 1,
                drag: None,
                vector_drag: None,
                scroll_drag: None,
                view: ViewState::default(),
                data: Arc::new(control_data(panel)),
                pointer: None,
                held: false,
                edit: None,
                motion: motion::PanelMotion::default(),
                motion_data_dirty: false,
                text_events: Vec::new(),
                field_events: Vec::new(),
                prepared,
            });
        }
        if (self.reduced_motion || !panel.motion)
            && let Some(current) = self.mod_panel.as_mut()
        {
            current.snap_motion();
        }
        Ok(())
    }

    /// Changes visibility once per transition and restores animated feedback when closing.
    /// Closing releases input at once; a painted surface whose transitions declare exit values
    /// keeps drawing until they settle, including authored HUD editor chrome.
    /// Reopening during that exit eases back from the displayed values.
    pub fn set_mod_panel_open(&mut self, open: bool) {
        let editor_owned = self.mod_hud_editor_open();
        let exit = !self.reduced_motion && !editor_owned;
        if !open && let Some(editor) = self.mod_hud_editor.as_mut() {
            editor.close(!self.reduced_motion);
        }
        if let Some(panel) = self.mod_panel.as_mut() {
            if open {
                panel.open = true;
                panel.motion.transitions.cancel_exit();
                if self.reduced_motion || !panel.panel.motion {
                    panel.snap_motion();
                }
            } else {
                panel.close(exit);
            }
        }
    }

    /// Whether the panel is open and owns input; false while a closed panel plays its exit.
    pub fn mod_panel_open(&self) -> bool {
        self.mod_panel.as_ref().is_some_and(|panel| panel.open)
    }

    /// Whether the active panel retains resource-pack HUD content for placement or inspection.
    pub fn mod_panel_shows_gameplay_hud(&self) -> bool {
        self.mod_hud_editor_painting()
            || self
                .mod_panel
                .as_ref()
                .is_some_and(|panel| panel.open && panel.panel.show_gameplay_hud)
    }

    /// Whether focused keys are owned by guest behavior instead of the native close shortcut.
    pub fn mod_panel_capture_key(&self) -> bool {
        self.mod_panel
            .as_ref()
            .is_some_and(|panel| panel.panel.capture_key)
    }

    /// Whether the panel draws: open, or closed while bindings ease to their exit values.
    pub fn mod_panel_painting(&self) -> bool {
        self.mod_hud_editor_painting()
            || self
                .mod_panel
                .as_ref()
                .is_some_and(|panel| panel.open || panel.exiting())
    }

    /// Native feedback, surface transitions and exits need frames only while they are moving.
    pub fn mod_panel_motion_active(&self) -> bool {
        self.mod_hud_editor_motion_active()
            || self.mod_panel.as_ref().is_some_and(|panel| {
                panel.exiting()
                    || panel.open
                        && (panel.motion.active()
                            || panel.motion_data_dirty && panel.animated()
                            || matches!(panel.edit, Some(edit::Editor::Field { .. })))
            })
    }

    /// Explicitly overrides mod surface motion, independently of built-in screen preferences.
    /// Disabling settles transitions, exits and opted-in feedback, including values already moving.
    pub fn configure_motion(&mut self, enabled: bool) {
        let reduced_motion = !enabled;
        if self.reduced_motion == reduced_motion {
            return;
        }
        self.reduced_motion = reduced_motion;
        if !enabled && let Some(panel) = self.mod_panel.as_mut() {
            panel.snap_motion();
        }
        if !enabled && let Some(editor) = self.mod_hud_editor.as_mut().filter(|editor| !editor.open)
        {
            editor.stop_exit();
        }
    }

    pub fn mod_panel_toggle_key(&self) -> Option<&str> {
        self.mod_panel
            .as_ref()
            .map(|panel| panel.panel.toggle_key.as_str())
    }

    /// Events use only geometry from the last rendered frame. Slider drags stay captured.
    pub fn mod_panel_events(
        &mut self,
        position: [f32; 2],
        pressed: bool,
        held: bool,
    ) -> Vec<Event> {
        let controls = self
            .mod_panel
            .as_ref()
            .map_or(&[][..], |panel| panel.panel.controls.as_slice());
        if let Some(editor) = self.mod_hud_editor.as_mut().filter(|e| e.open) {
            let events = editor.pointer(position, pressed, held, controls);
            if editor.close_requested {
                self.set_mod_panel_open(false);
            }
            return events;
        }
        let exit = !self.reduced_motion;
        self.mod_panel.as_mut().map_or_else(Vec::new, |panel| {
            panel.pointer_events(position, pressed, held, exit)
        })
    }

    /// Draws the open or exiting panel, fitted to its authored reference size; the caller draws
    /// the HUD editor instead while it owns the panel surface.
    pub fn paint_mod_panel(
        &mut self,
        engine: Option<&FormEngine>,
        paint_frame: &mut PaintFrame<'_>,
    ) {
        self.paint_mod_panel_with_icons(engine, paint_frame, &|_, _| None);
    }

    /// Paints live previews using the caller's current item atlas, resolving only requested rows.
    pub fn paint_mod_panel_with_icons(
        &mut self,
        engine: Option<&FormEngine>,
        paint_frame: &mut PaintFrame<'_>,
        icon: &dyn Fn(&str, u32) -> Option<ui::IconRef>,
    ) {
        let hud_metrics = paint_frame.metrics;
        let animate = !self.reduced_motion;
        let Some(panel) = self
            .mod_panel
            .as_mut()
            .filter(|panel| panel.open || panel.exiting())
        else {
            return;
        };
        let Some(renderer) = engine else {
            panel.close(false);
            return;
        };
        let content = paint_frame.content;
        // Authored surfaces may declare their logical extent for viewport fitting.
        let scale = panel.panel.reference_size.map_or(1.0, |size| {
            content
                .into_iter()
                .zip(size)
                .map(|(available, required)| {
                    available / (required * FONT_DESIGN_PIXEL_TEXELS as f32)
                })
                .fold(1.0, f32::min)
                .clamp(UiScale::DISPLAY_MIN, 1.0)
        });
        paint_frame.metrics.scale =
            UiScale::new_display(scale).expect("fitted display scale is valid");
        let px = paint_frame.metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        let viewport = [f64::from(content[0] / px), f64::from(content[1] / px)];
        if viewport[0] < 120.0
            || viewport[1] - layout::top_offset(viewport) - 12.0 < CHROME_HEIGHT + ROW_HEIGHT + 8.0
        {
            panel.close(false);
            return;
        }
        let rows = (((viewport[1] - layout::top_offset(viewport) - CHROME_HEIGHT - 12.0)
            / ROW_HEIGHT) as usize)
            .clamp(1, ui::mod_panel::MAX_PANEL_CONTROLS);
        if panel.viewport != viewport || panel.rows != rows {
            panel.cancel_edit();
            panel.viewport = viewport;
            panel.refresh_data();
            panel.rows = rows;
            panel.page = panel.page.min(panel.last_page());
            if panel.panel.surface.is_none() {
                panel.catalog = None;
            }
            panel.frame = None;
            panel.drag = None;
            panel.vector_drag = None;
            panel.scroll_drag = None;
            panel.pointer = None;
        }
        if panel.catalog.is_none() && panel.edit.is_none() {
            panel.catalog = panel
                .prepared
                .as_ref()
                .map(|route| Arc::clone(&route.catalog));
        }
        if panel.catalog.is_none() {
            match template::catalog_prepared(
                &panel.panel,
                viewport,
                panel.category,
                panel.page,
                rows,
                panel.edit.as_ref(),
                panel.prepared.as_ref().map(|route| route.catalog.as_ref()),
            ) {
                Ok((catalog, pages)) => {
                    panel.pages = pages;
                    panel.page = panel.page.min(pages - 1);
                    panel.catalog = Some(Arc::new(catalog));
                    panel.screen = CachedScreen::default();
                }
                Err(error) => {
                    panel.close(false);
                    tracing::warn!(%error, "personal extension panel rejected");
                    return;
                }
            }
        }
        let changed = panel.advance_motion(paint_frame.now, animate);
        if !panel.open && !panel.motion.transitions.active() {
            // The exit has settled: remove the surface without drawing its final values.
            panel.close(false);
            return;
        }
        if panel.animated() && (changed || panel.motion_data_dirty) {
            panel
                .motion
                .write(&panel.panel, Arc::make_mut(&mut panel.data));
            panel.motion_data_dirty = false;
        }
        let data = Arc::clone(&panel.data);
        let rollback = (paint_frame.nodes.len(), *paint_frame.next);
        let world = panel
            .panel
            .surface
            .as_ref()
            .map(|surface| panel.motion.transitions.world_backdrop(surface))
            .unwrap_or_default();
        if let Err(error) = paint_world_backdrop(paint_frame, world) {
            out_of_render(panel, paint_frame, rollback);
            tracing::warn!(%error, "personal extension panel disabled after rendering failure");
            return;
        }
        let caller = std::mem::take(&mut paint_frame.renderers);
        let result = {
            let mut glass = GlassRenderer::new(world);
            let editing = panel.edit.is_some();
            let mut field = field_paint::FieldRenderer::new(
                panel.edit.as_mut(),
                paint_frame.now,
                Palette::for_panel(&panel.panel),
                [paint_frame.safe_area.left(), paint_frame.safe_area.top()],
            );
            let mut field_slot: [&mut dyn CustomRenderer; 1] = [&mut field];
            let mut host = RendererChain {
                first: &mut glass,
                rest: &mut field_slot,
            };
            let mut base = RendererChain {
                first: &mut host,
                rest: &mut *caller,
            };
            let language = paint_frame.language;
            let art = ScreenArt {
                view: Some(&panel.view),
                ..Default::default()
            };
            let catalog = panel.catalog.as_ref().expect("catalog installed above");
            let screen = panel
                .prepared
                .as_ref()
                .filter(|_| !editing)
                .and_then(|route| {
                    self.prepared_panel_ui
                        .get_mut(&panel.owner)
                        .map(|owner| owner.screen(route))
                })
                .unwrap_or(&mut panel.screen);
            if let Some(surface) = &panel.panel.surface {
                for name in surface.collections.keys() {
                    screen.virtualize_clipped_grid(
                        name,
                        ui::mod_panel::SurfaceCollection::INITIAL_WINDOW,
                    );
                }
            }
            let laid = {
                let mut renderers: [&mut dyn CustomRenderer; 1] = [&mut base];
                renderer
                    .prepare(
                        art,
                        &mut paint_frame.with_renderers(&mut renderers),
                        |env, root| {
                            screen.render_shared_with(
                                SCREEN,
                                catalog,
                                crate::personal_context(),
                                Arc::clone(&data),
                                (root, px, language),
                                env,
                                &panel.view,
                            )
                        },
                    )
                    .map(|render| Arc::clone(&render.nodes))
            };
            if let Some(laid) = laid {
                let panel_metrics = paint_frame.metrics;
                paint_frame.metrics = hud_metrics;
                panel.hud_previews.prepare(
                    &laid,
                    &panel.view,
                    panel.owner,
                    &mut self.hud_sources,
                    renderer,
                    paint_frame,
                    icon,
                );
                paint_frame.metrics = panel_metrics;
            }
            let mut renderers: [&mut dyn CustomRenderer; 1] = [&mut base];
            let mut previews = panel
                .hud_previews
                .renderer(panel.owner, &mut self.hud_sources);
            let mut combined = RendererChain {
                first: &mut previews,
                rest: &mut renderers,
            };
            let mut preview_renderers: [&mut dyn CustomRenderer; 1] = [&mut combined];
            renderer.draw(
                art,
                &mut paint_frame.with_renderers(&mut preview_renderers),
                |env, root| {
                    screen.render_shared_with(
                        SCREEN,
                        catalog,
                        crate::personal_context(),
                        data,
                        (root, px, language),
                        env,
                        &panel.view,
                    )
                },
            )
        };
        paint_frame.renderers = caller;
        match result {
            Ok(Some(mut frame)) => {
                if let Some(surface) = &panel.panel.surface
                    && !surface.collections.is_empty()
                {
                    surface::resolve_collection_hits(surface, Arc::make_mut(&mut frame.hits));
                }
                for (key, metrics) in &frame.report.scrolls {
                    if let Some(offset) = panel.view.scroll.get_mut(key) {
                        *offset = metrics.offset;
                    }
                }
                panel.frame = Some(frame);
            }
            result => {
                out_of_render(panel, paint_frame, rollback);
                if let Err(error) = result {
                    tracing::warn!(%error, "personal extension panel disabled after rendering failure");
                }
            }
        }
    }
}

/// Borrows a host renderer and the caller's renderers without allocating a dispatch list.
struct RendererChain<'a, 'b> {
    first: &'a mut dyn CustomRenderer,
    rest: &'a mut [&'b mut dyn CustomRenderer],
}

impl CustomRenderer for RendererChain<'_, '_> {
    /// Tries the host renderer first, then the caller's borrowed renderers.
    fn paint(
        &mut self,
        node: &ui_engine::engine::screen::CustomDraw<'_>,
        canvas: &mut ui_engine::engine::Canvas<'_>,
    ) -> Result<bool, ui_engine::engine::screen::PaintError> {
        if self.first.paint(node, canvas)? {
            return Ok(true);
        }
        for renderer in self.rest.iter_mut() {
            if renderer.paint(node, canvas)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Preserves every renderer's texture residency declarations.
    fn textures<'s>(&'s self, drawn: &dyn Fn(&str) -> bool, texture: &mut dyn FnMut(&'s str)) {
        self.first.textures(drawn, texture);
        for renderer in self.rest.iter() {
            renderer.textures(drawn, texture);
        }
    }
}

fn out_of_render(panel: &mut ModPanel, paint_frame: &mut PaintFrame<'_>, rollback: (usize, u32)) {
    paint_frame.nodes.truncate(rollback.0);
    *paint_frame.next = rollback.1;
    panel.close(false);
}

impl ModPanel {
    /// Samples native feedback and surface transitions under both animation preferences.
    fn advance_motion(&mut self, now: f64, animate: bool) -> bool {
        let animate = animate && self.panel.motion;
        let mut changed = false;
        if self.panel.motion {
            if let Some(frame) = &self.frame {
                for (key, metrics) in &frame.report.scrolls {
                    self.motion.scroll_extent(key, metrics.max_offset());
                }
            }
            let hovered = self
                .frame
                .as_ref()
                .and_then(|frame| {
                    frame
                        .hits
                        .iter()
                        .find(|hit| self.view.hovered.as_deref() == Some(hit.key.as_str()))
                })
                .and_then(|hit| hit.pressed.as_deref())
                .and_then(|action| {
                    action
                        .strip_prefix("mod.control:")
                        .or_else(|| action.strip_prefix("mod.edit:"))
                })
                .and_then(|index| index.parse().ok());
            changed = self
                .motion
                .advance(&self.panel, &mut self.view, hovered, now, animate);
        }
        if let Some(surface) = &self.panel.surface {
            changed |= self.motion.transitions.advance(surface, now, animate);
        }
        changed
    }

    /// Completes pending feedback and surface motion without restarting the current entrance.
    /// A closed panel's pending exit is removed immediately instead of being retained for paint.
    fn snap_motion(&mut self) {
        if !self.open {
            self.close(false);
            return;
        }
        let mut changed =
            self.motion
                .advance(&self.panel, &mut self.view, None, self.motion.now, false);
        if let Some(surface) = &self.panel.surface {
            changed |= self
                .motion
                .transitions
                .advance(surface, self.motion.now, false);
        }
        if changed || self.motion_data_dirty {
            self.motion
                .write(&self.panel, Arc::make_mut(&mut self.data));
        }
        self.motion_data_dirty = false;
    }

    /// Closes an open panel once, releasing input at once. With `exit`, a painted surface whose
    /// transitions declare exit values keeps its frame and scroll while they ease there; the
    /// panel is otherwise removed at once, clearing scroll state and animated feedback. Without
    /// `exit`, this also ends an exit in progress, which a repeated animated close continues.
    fn close(&mut self, exit: bool) {
        let exit = exit && self.panel.motion;
        if !self.open && (exit || !self.exiting()) {
            return;
        }
        let reset_feedback = self.motion.needs_feedback_reset() || self.exiting();
        if self.open {
            self.open = false;
            self.cancel_edit();
            self.cancel_pointer_input();
            if exit
                && let Some(surface) = &self.panel.surface
                && self.motion.transitions.begin_exit(surface)
            {
                self.view.focused = None;
                self.motion_data_dirty = true;
                return;
            }
        }
        self.frame = None;
        self.view = ViewState::default();
        self.motion = motion::PanelMotion::default();
        if reset_feedback {
            self.refresh_data();
        }
    }

    /// Whether the closed panel still draws while bindings ease to their exit values.
    fn exiting(&self) -> bool {
        !self.open && self.motion.transitions.exiting()
    }

    /// Synchronizes disabled feedback after local edits, then publishes values and viewport size.
    fn refresh_data(&mut self) {
        if !self.panel.motion {
            self.motion
                .advance(&self.panel, &mut self.view, None, self.motion.now, false);
        }
        let mut data = control_data(&self.panel);
        if self.panel.surface.is_some() {
            surface::viewport(&mut data, self.viewport);
        }
        self.data = Arc::new(data);
        self.motion_data_dirty = true;
    }

    /// Whether native feedback or declared surface transitions rewrite published bindings.
    fn animated(&self) -> bool {
        self.panel.motion
            || self
                .panel
                .surface
                .as_ref()
                .is_some_and(|surface| !surface.transitions.is_empty())
    }

    fn last_page(&self) -> usize {
        self.pages.saturating_sub(1)
    }
}

#[cfg(any(test, feature = "test-support"))]
impl ModUi {
    pub fn mod_panel_state(&self) -> Option<crate::test_support::ModPanelState<'_>> {
        let panel = self.mod_panel.as_ref()?;
        let screen = panel
            .prepared
            .as_ref()
            .filter(|_| panel.edit.is_none())
            .and_then(|route| {
                self.prepared_panel_ui
                    .get(&panel.owner)?
                    .cached_screen(route)
            })
            .unwrap_or(&panel.screen);
        Some(crate::test_support::ModPanelState {
            catalog: panel.catalog.as_ref(),
            frame: panel.frame.as_ref(),
            page: panel.page,
            rows: panel.rows,
            category: panel.category,
            pages: panel.pages,
            passes: screen.passes,
            #[cfg(feature = "test-support")]
            resolves: screen.resolves,
        })
    }
}

#[cfg(test)]
mod collection_tests;
#[cfg(test)]
mod motion_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod text_tests;
