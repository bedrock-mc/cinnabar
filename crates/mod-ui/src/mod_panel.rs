//! A private JSON-UI catalog for bounded personal-extension controls.

mod compact;
mod data;
mod edit;
mod icons;
mod input;
mod layout;
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

use crate::ModUi;
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
}

impl ModUi {
    /// Drops the panel's laid-out text and pointer capture after its private font changed.
    pub fn invalidate_mod_panel_font(&mut self) {
        self.cancel_mod_hud_editor();
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

    /// Replaces validated control data, retaining the open state across value updates.
    pub fn set_mod_panel(&mut self, panel: Option<&Panel>) -> Result<(), String> {
        let Some(panel) = panel else {
            self.cancel_mod_hud_editor();
            self.mod_panel = None;
            return Ok(());
        };
        panel.validate()?;
        if let Some(current) = self.mod_panel.as_mut() {
            if current.panel == *panel {
                return Ok(());
            }
            if !template::same_shape(&current.panel, panel) {
                current.edit = None;
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
                current.pointer = None;
            }
            if panel.capture_key {
                current.cancel_edit();
            }
            current.panel = panel.clone();
            current.refresh_data();
        } else {
            self.mod_panel = Some(ModPanel {
                panel: panel.clone(),
                open: false,
                catalog: None,
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
            });
        }
        Ok(())
    }

    pub fn set_mod_panel_open(&mut self, open: bool) {
        if !open {
            self.cancel_mod_hud_editor();
        }
        if let Some(panel) = self.mod_panel.as_mut() {
            panel.open = open;
            if !open {
                panel.cancel_edit();
                panel.frame = None;
                panel.drag = None;
                panel.vector_drag = None;
                panel.scroll_drag = None;
                panel.view = ViewState::default();
                panel.pointer = None;
            }
        }
    }

    pub fn mod_panel_open(&self) -> bool {
        self.mod_panel.as_ref().is_some_and(|panel| panel.open)
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
            panel.open = false;
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
            panel.cancel_edit();
            panel.frame = None;
            panel.open = false;
            panel.drag = None;
            panel.vector_drag = None;
            panel.scroll_drag = None;
            panel.pointer = None;
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
            panel.catalog = None;
            panel.frame = None;
            panel.drag = None;
            panel.vector_drag = None;
            panel.scroll_drag = None;
            panel.pointer = None;
        }
        if panel.catalog.is_none() {
            match template::catalog(
                &panel.panel,
                viewport,
                panel.category,
                panel.page,
                rows,
                panel.edit.as_ref(),
            ) {
                Ok((catalog, pages)) => {
                    panel.pages = pages;
                    panel.page = panel.page.min(pages - 1);
                    panel.catalog = Some(Arc::new(catalog));
                    panel.screen = CachedScreen::default();
                }
                Err(error) => {
                    panel.open = false;
                    tracing::warn!(%error, "personal extension panel rejected");
                    return;
                }
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
        match renderer.draw(art, paint_frame, |env, root| {
            panel.screen.render_shared_with(
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
    panel.cancel_edit();
    panel.frame = None;
    panel.drag = None;
    panel.scroll_drag = None;
    panel.open = false;
    panel.pointer = None;
}

impl ModPanel {
    /// Publishes typed control values and host viewport dimensions together.
    fn refresh_data(&mut self) {
        let mut data = control_data(&self.panel);
        if self.panel.surface.is_some() {
            surface::viewport(&mut data, self.viewport);
        }
        self.data = Arc::new(data);
    }

    fn last_page(&self) -> usize {
        self.pages.saturating_sub(1)
    }
}

#[cfg(any(test, feature = "test-support"))]
impl ModUi {
    pub fn mod_panel_state(&self) -> Option<crate::test_support::ModPanelState<'_>> {
        let panel = self.mod_panel.as_ref()?;
        Some(crate::test_support::ModPanelState {
            catalog: panel.catalog.as_ref(),
            frame: panel.frame.as_ref(),
            page: panel.page,
            rows: panel.rows,
            category: panel.category,
            pages: panel.pages,
            passes: panel.screen.passes,
        })
    }
}

#[cfg(test)]
mod tests;
