//! Native layout editing of bounded HUD previews. Pointer data never enters a guest.

#[cfg(test)]
mod autosave_tests;
mod input;
mod template;
#[cfg(test)]
mod tests;

use super::{
    engine::{EngineInputs, EngineOutput, ScreenArt},
    hud::CachedScreen,
    mod_widgets::template as cards,
};
use crate::ui_runtime::{UiRuntime, forms::EngineFrame};
use json_ui::{Catalog, Context, DataSource, Scalar, ViewState};
use std::sync::Arc;
use ui::{
    IconRef, UiNode,
    mod_hud::{EditorResult, Hud, MAX_HUD_ROWS},
};
use {
    super::super::{TextMetrics, UiPresentationRuntime},
    ui::FONT_DESIGN_PIXEL_TEXELS,
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
}

impl UiPresentationRuntime {
    /// Opens bounded previews independently of whether their gameplay cards are enabled.
    pub fn open_mod_hud_editor(&mut self, preview: &Hud) -> Result<(), String> {
        preview.validate()?;
        if !self.mod_panel_open() {
            return Err("HUD editor requires an open panel".into());
        }
        self.cancel_mod_panel_edit();
        self.form_presentation.mod_hud_editor = Some(HudEditor {
            draft: preview.clone(),
            committed: preview.clone(),
            autosave: preview.autosave,
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
        });
        Ok(())
    }

    /// Whether native layout editing currently owns the personal panel surface.
    pub fn mod_hud_editor_open(&self) -> bool {
        self.form_presentation
            .mod_hud_editor
            .as_ref()
            .is_some_and(|editor| editor.open)
    }

    /// Cancels a draft on ownership, focus, session or presentation loss.
    pub fn cancel_mod_hud_editor(&mut self) {
        if let Some(editor) = self
            .form_presentation
            .mod_hud_editor
            .as_mut()
            .filter(|e| e.open)
        {
            editor.finish(false);
        }
    }

    /// Host-owned result, delivered only to the component that requested this draft.
    pub fn take_mod_hud_editor_result(&mut self) -> Option<EditorResult> {
        let editor = self.form_presentation.mod_hud_editor.as_mut()?;
        let result = editor.result.take()?;
        if !editor.open {
            self.form_presentation.mod_hud_editor = None;
        }
        Some(result)
    }

    /// Draws previews and editor chrome at the same GUI scale as gameplay cards.
    pub(in super::super) fn append_mod_hud_editor(
        &mut self,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        content: [f32; 2],
    ) {
        let Some(editor) = self
            .form_presentation
            .mod_hud_editor
            .as_ref()
            .filter(|e| e.open)
        else {
            return;
        };
        let mut resolved = [None; MAX_HUD_ROWS];
        let mut count = 0;
        for row in editor.draft.cards.iter().flat_map(|c| &c.rows) {
            resolved[count] = row
                .item
                .as_deref()
                .and_then(|id| self.item_icon(id, row.metadata));
            count += 1;
        }
        let Some(renderer) = self.form_presentation.engine.as_deref() else {
            self.cancel_mod_hud_editor();
            return;
        };
        let editor = self
            .form_presentation
            .mod_hud_editor
            .as_mut()
            .expect("checked above");
        let px = metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        let viewport = [f64::from(content[0] / px), f64::from(content[1] / px)];
        if viewport[0] < 220. || viewport[1] < 120. {
            editor.finish(false);
            return;
        }
        if editor.viewport != viewport {
            editor.viewport = viewport;
            editor.catalog = None;
            editor.frame = None;
            editor.cancel_pointer_input();
        }
        if editor.catalog.is_none() {
            match template::catalog(editor, viewport) {
                Ok(catalog) => {
                    editor.catalog = Some(Arc::new(catalog));
                    editor.screen = CachedScreen::default();
                }
                Err(error) => {
                    editor.finish(false);
                    bevy::log::warn!(%error, "HUD editor template rejected");
                    return;
                }
            }
        }
        let mut data = editor.data();
        let mut icons = Vec::<IconRef>::new();
        for (index, icon) in resolved[..count].iter().enumerate() {
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
            icons: &icons,
            view: Some(&editor.view),
            ..Default::default()
        };
        let result = renderer.draw(art, inputs, out, |env, root| {
            editor.screen.render_shared_with(
                SCREEN,
                editor.catalog.as_ref().expect("installed"),
                &Context::default(),
                Arc::clone(&editor.data),
                (root, px, runtime.text_generation()),
                env,
                &editor.view,
            )
        });
        match result {
            Ok(Some(frame)) => editor.frame = Some(frame),
            result => {
                nodes.truncate(rollback.0);
                *next = rollback.1;
                editor.finish(false);
                if let Err(error) = result {
                    bevy::log::warn!(%error, "HUD editor disabled after rendering failure");
                }
            }
        }
    }
}

impl HudEditor {
    /// Binds changing positions and selection without regenerating the catalog.
    fn data(&self) -> DataSource {
        let mut data = cards::data(&self.draft, self.viewport);
        data.set_global("#grid_visible", Scalar::Bool(self.snap));
        for axis in 0..2 {
            let guide = self.drag.as_ref().and_then(|drag| drag.guides[axis]);
            data.set_global(
                format!("#guide_{axis}_visible"),
                Scalar::Bool(guide.is_some()),
            );
            let mut at = [0.; 2];
            at[axis] = guide.unwrap_or(0.);
            data.set_global(
                format!("#guide_{axis}_offset"),
                Scalar::Json(serde_json::json!(at)),
            );
        }
        if let Some(surface) = &self.draft.surface {
            super::mod_panel::surface::bind(surface, &mut data);
            super::mod_panel::surface::viewport(&mut data, self.viewport);
        }
        data.set_global(
            "#grid_label",
            Scalar::Text(if self.snap { "Grid: ON" } else { "Grid: OFF" }.into()),
        );
        for (index, card) in self.draft.cards.iter().enumerate() {
            let at = cards::origin(card, self.viewport);
            let size = cards::dimensions(card);
            let label_y = if at[1] >= 12. {
                at[1] - 12.
            } else {
                at[1] + size[1] + 2.
            };
            let label_y = label_y.clamp(0., (self.viewport[1] - 12.).max(0.));
            data.set_global(
                format!("#bounds_{index}_label_offset"),
                Scalar::Json(serde_json::json!([at[0], label_y])),
            );
            data.set_global(
                format!("#bounds_{index}_color"),
                Scalar::Json(serde_json::json!(if self.selected == Some(index) {
                    [0.3, 0.85, 1., 1.]
                } else {
                    [1., 1., 1., 0.55]
                })),
            );
        }
        data
    }
}
