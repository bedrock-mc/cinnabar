//! A private JSON-UI catalog for bounded personal-extension controls.

mod compact;
mod data;
mod icons;
mod input;
mod layout;
mod template;
#[cfg(test)]
mod tests;
mod widgets;

use std::sync::Arc;

use json_ui::{Catalog, Context, DataSource, ViewState};
use ui::{
    UiNode, UiScale,
    mod_panel::{Control, Event, Panel},
};

use super::super::{FONT_DESIGN_PIXEL_TEXELS, TextMetrics, UiPresentationRuntime};
use super::{
    engine::{EngineInputs, EngineOutput, ScreenArt},
    hud::CachedScreen,
};
use crate::ui_runtime::{UiRuntime, forms::EngineFrame};
use data::control_data;

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
    view: ViewState,
    data: Arc<DataSource>,
    pointer: Option<[f32; 2]>,
    held: bool,
}

impl UiPresentationRuntime {
    pub(in super::super) fn invalidate_mod_panel_font(&mut self) {
        if let Some(panel) = &mut self.form_presentation.mod_panel {
            panel.screen = CachedScreen::default();
            panel.frame = None;
            panel.drag = None;
            panel.pointer = None;
        }
    }

    /// Replaces validated control data, retaining the open state across value updates.
    pub fn set_mod_panel(&mut self, panel: Option<&Panel>) -> Result<(), String> {
        let Some(panel) = panel else {
            self.form_presentation.mod_panel = None;
            return Ok(());
        };
        panel.validate()?;
        if let Some(current) = self.form_presentation.mod_panel.as_mut() {
            if current.panel == *panel {
                return Ok(());
            }
            if !template::same_shape(&current.panel, panel) {
                current.catalog = None;
                current.frame = None;
                current.drag = None;
                if current.panel.sections != panel.sections
                    || current.panel.controls.len() != panel.controls.len()
                {
                    current.page = 0;
                    current.category = 0;
                }
                current.view = ViewState::default();
                current.pointer = None;
            }
            current.panel = panel.clone();
            current.data = Arc::new(control_data(panel));
        } else {
            self.form_presentation.mod_panel = Some(ModPanel {
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
                view: ViewState::default(),
                data: Arc::new(control_data(panel)),
                pointer: None,
                held: false,
            });
        }
        Ok(())
    }

    pub fn set_mod_panel_open(&mut self, open: bool) {
        if let Some(panel) = self.form_presentation.mod_panel.as_mut() {
            panel.open = open;
            if !open {
                panel.frame = None;
                panel.drag = None;
                panel.view = ViewState::default();
                panel.pointer = None;
            }
        }
    }

    pub fn mod_panel_open(&self) -> bool {
        self.form_presentation
            .mod_panel
            .as_ref()
            .is_some_and(|panel| panel.open)
    }

    pub fn mod_panel_toggle_key(&self) -> Option<&str> {
        self.form_presentation
            .mod_panel
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
        self.form_presentation
            .mod_panel
            .as_mut()
            .map_or_else(Vec::new, |panel| {
                panel.pointer_events(position, pressed, held)
            })
    }

    pub(in super::super) fn append_mod_panel(
        &mut self,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        mut metrics: TextMetrics,
        content: [f32; 2],
    ) {
        let Some(panel) = self
            .form_presentation
            .mod_panel
            .as_mut()
            .filter(|panel| panel.open)
        else {
            return;
        };
        let Some(renderer) = self.form_presentation.engine.as_deref() else {
            panel.open = false;
            return;
        };
        // Desktop controls retain logical sizing; the render tree applies platform DPI.
        metrics.scale = UiScale::new_display(1.0).expect("unit display scale is valid");
        let px = metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        let viewport = [f64::from(content[0] / px), f64::from(content[1] / px)];
        if viewport[0] < 120.0
            || viewport[1] - layout::top_offset(viewport) - 12.0 < CHROME_HEIGHT + ROW_HEIGHT + 8.0
        {
            panel.frame = None;
            panel.open = false;
            panel.drag = None;
            panel.pointer = None;
            return;
        }
        let rows = (((viewport[1] - layout::top_offset(viewport) - CHROME_HEIGHT - 12.0)
            / ROW_HEIGHT) as usize)
            .clamp(1, ui::mod_panel::MAX_PANEL_CONTROLS);
        if panel.viewport != viewport || panel.rows != rows {
            panel.viewport = viewport;
            panel.rows = rows;
            panel.page = panel.page.min(panel.last_page());
            panel.catalog = None;
            panel.frame = None;
            panel.drag = None;
            panel.pointer = None;
        }
        if panel.catalog.is_none() {
            match template::catalog(&panel.panel, viewport, panel.category, panel.page, rows) {
                Ok((catalog, pages)) => {
                    panel.pages = pages;
                    panel.page = panel.page.min(pages - 1);
                    panel.catalog = Some(Arc::new(catalog));
                    panel.screen = CachedScreen::default();
                }
                Err(error) => {
                    panel.open = false;
                    bevy::log::warn!(%error, "personal extension panel rejected");
                    return;
                }
            }
        }
        let data = Arc::clone(&panel.data);
        let inputs = EngineInputs {
            layouts: &mut self.layouts,
            font: &self.font,
            metrics,
            solid_page: self.solid_texture_page,
            safe_area: self.safe_area,
            content,
            translate: &|_| None,
            language: runtime.text_generation(),
        };
        let rollback = (nodes.len(), *next);
        let out = EngineOutput {
            nodes: &mut *nodes,
            next: &mut *next,
            overlay: &[],
        };
        let art = ScreenArt {
            view: Some(&panel.view),
            ..Default::default()
        };
        let catalog = panel.catalog.as_ref().expect("catalog installed above");
        match renderer.draw(art, inputs, out, |env, root| {
            panel.screen.render_shared_with(
                SCREEN,
                catalog,
                &Context::default(),
                data,
                (root, px, runtime.text_generation()),
                env,
                &panel.view,
            )
        }) {
            Ok(Some(frame)) => panel.frame = Some(frame),
            result => {
                out_of_render(panel, nodes, next, rollback);
                if let Err(error) = result {
                    bevy::log::warn!(%error, "personal extension panel disabled after rendering failure");
                }
            }
        }
    }
}

fn out_of_render(
    panel: &mut ModPanel,
    nodes: &mut Vec<UiNode>,
    next: &mut u32,
    rollback: (usize, u32),
) {
    nodes.truncate(rollback.0);
    *next = rollback.1;
    panel.frame = None;
    panel.drag = None;
    panel.open = false;
    panel.pointer = None;
}

impl ModPanel {
    fn last_page(&self) -> usize {
        self.pages.saturating_sub(1)
    }
}
