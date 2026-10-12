//! A private JSON-UI catalog for bounded personal-extension controls.

mod compact;
mod data;
mod edit;
mod icons;
mod input;
mod layout;
mod motion;
pub(crate) mod prepared;
mod scrolling;
pub(crate) mod surface;
mod template;
mod vector;
mod widgets;

#[cfg(any(test, feature = "test-support"))]
pub(crate) use {template::same_shape, widgets::Palette};

use std::sync::Arc;

use json_ui::{Catalog, Context, DataSource, ViewState};
use ui::{
    FONT_DESIGN_PIXEL_TEXELS, UiScale,
    mod_panel::{Control, Event, Panel},
};

use crate::{ModUi, PanelUiOwner};
use data::control_data;
use ui_engine::frame::EngineFrame;
use ui_engine::{
    cached_screen::CachedScreen,
    engine::{FormEngine, ScreenArt, screen::PaintFrame},
};

const SCREEN: &str = "cinnabar_personal.panel";
const ROW_HEIGHT: f64 = 26.0;
const CHROME_HEIGHT: f64 = 88.0;

pub(super) struct ModPanel {
    owner: PanelUiOwner,
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
    vector_drag: Option<String>,
    scroll_drag: Option<(String, f64)>,
    view: ViewState,
    data: Arc<DataSource>,
    pointer: Option<[f32; 2]>,
    held: bool,
    edit: Option<edit::Editor>,
    motion: motion::PanelMotion,
    motion_data_dirty: bool,
    prepared: Option<Arc<prepared::PreparedRoute>>,
}

impl ModUi {
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
        if let Some(current) = self.mod_panel.as_mut() {
            if !same_shape {
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
                current.pointer = None;
                current.prepared = prepared.clone();
                current.catalog = prepared.as_ref().map(|route| Arc::clone(&route.catalog));
            }
            if panel.capture_key {
                current.cancel_edit();
            }
            current.panel = panel.clone();
            current.refresh_data();
        } else {
            self.mod_panel = Some(ModPanel {
                owner,
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
                prepared,
            });
        }
        Ok(())
    }

    /// Changes visibility once per transition and restores animated feedback when closing.
    pub fn set_mod_panel_open(&mut self, open: bool) {
        if !open {
            self.cancel_mod_hud_editor();
        }
        if let Some(panel) = self.mod_panel.as_mut() {
            if open {
                panel.open = true;
            } else {
                panel.close();
            }
        }
    }

    pub fn mod_panel_open(&self) -> bool {
        self.mod_panel.as_ref().is_some_and(|panel| panel.open)
    }

    /// Native feedback needs frames only while the opt-in panel motion is active.
    pub fn mod_panel_motion_active(&self) -> bool {
        self.mod_panel.as_ref().is_some_and(|panel| {
            panel.open && panel.panel.motion && (panel.motion_data_dirty || panel.motion.active())
        })
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
        self.mod_panel.as_mut().map_or_else(Vec::new, |panel| {
            panel.pointer_events(position, pressed, held)
        })
    }

    /// Draws the open panel, fitted to its authored reference size; the caller draws the HUD
    /// editor instead while it owns the panel surface.
    pub fn paint_mod_panel(
        &mut self,
        engine: Option<&FormEngine>,
        paint_frame: &mut PaintFrame<'_>,
    ) {
        let Some(panel) = self.mod_panel.as_mut().filter(|panel| panel.open) else {
            return;
        };
        let Some(renderer) = engine else {
            panel.close();
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
            panel.close();
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
                    panel.close();
                    tracing::warn!(%error, "personal extension panel rejected");
                    return;
                }
            }
        }
        if panel.panel.motion {
            if let Some(frame) = &panel.frame {
                for (key, metrics) in &frame.report.scrolls {
                    panel.motion.scroll_extent(key, metrics.max_offset());
                }
            }
            let hovered = panel
                .frame
                .as_ref()
                .and_then(|frame| {
                    frame
                        .hits
                        .iter()
                        .find(|hit| panel.view.hovered.as_deref() == Some(hit.key.as_str()))
                })
                .and_then(|hit| hit.pressed.as_deref())
                .and_then(|action| {
                    action
                        .strip_prefix("mod.control:")
                        .or_else(|| action.strip_prefix("mod.edit:"))
                })
                .and_then(|index| index.parse().ok());
            let changed =
                panel
                    .motion
                    .advance(&panel.panel, &mut panel.view, hovered, paint_frame.now);
            if changed || panel.motion_data_dirty {
                panel
                    .motion
                    .write(&panel.panel, Arc::make_mut(&mut panel.data));
                panel.motion_data_dirty = false;
            }
        }
        let data = Arc::clone(&panel.data);
        let rollback = (paint_frame.nodes.len(), *paint_frame.next);
        let language = paint_frame.language;
        let art = ScreenArt {
            view: Some(&panel.view),
            ..Default::default()
        };
        let catalog = panel.catalog.as_ref().expect("catalog installed above");
        let screen = panel
            .prepared
            .as_ref()
            .filter(|_| panel.edit.is_none())
            .and_then(|route| {
                self.prepared_panel_ui
                    .get_mut(&panel.owner)
                    .map(|owner| owner.screen(route))
            })
            .unwrap_or(&mut panel.screen);
        match renderer.draw(art, paint_frame, |env, root| {
            screen.render_shared_with(
                SCREEN,
                catalog,
                &Context::default(),
                data,
                (root, px, language),
                env,
                &panel.view,
            )
        }) {
            Ok(Some(frame)) => {
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

fn out_of_render(panel: &mut ModPanel, paint_frame: &mut PaintFrame<'_>, rollback: (usize, u32)) {
    paint_frame.nodes.truncate(rollback.0);
    *paint_frame.next = rollback.1;
    panel.close();
}

impl ModPanel {
    /// Closes an open panel once, clearing input, scroll state, and animated feedback.
    fn close(&mut self) {
        if !self.open {
            return;
        }
        let reset_feedback = self.motion.needs_feedback_reset();
        self.open = false;
        self.cancel_edit();
        self.frame = None;
        self.drag = None;
        self.vector_drag = None;
        self.scroll_drag = None;
        self.view = ViewState::default();
        self.motion = motion::PanelMotion::default();
        self.pointer = None;
        self.held = false;
        if reset_feedback {
            self.refresh_data();
        }
    }

    /// Publishes typed control values and host viewport dimensions together.
    fn refresh_data(&mut self) {
        let mut data = control_data(&self.panel);
        if self.panel.surface.is_some() {
            surface::viewport(&mut data, self.viewport);
        }
        self.data = Arc::new(data);
        self.motion_data_dirty = true;
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
mod tests;
