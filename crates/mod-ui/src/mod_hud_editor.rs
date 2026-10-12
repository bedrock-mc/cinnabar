//! Native layout editing of bounded HUD previews. Pointer data never enters a guest.

mod input;
mod template;

use crate::{ModUi, RowIcons, mod_widgets::template as cards};
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
}

impl ModUi {
    /// Opens bounded previews independently of whether their gameplay cards are enabled.
    pub fn open_mod_hud_editor(&mut self, preview: &Hud) -> Result<(), String> {
        preview.validate()?;
        if !self.mod_panel_open() {
            return Err("HUD editor requires an open panel".into());
        }
        self.cancel_mod_panel_edit();
        self.mod_hud_editor = Some(HudEditor {
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
        self.mod_hud_editor
            .as_ref()
            .is_some_and(|editor| editor.open)
    }

    /// Cancels a draft on ownership, focus, session or presentation loss.
    pub fn cancel_mod_hud_editor(&mut self) {
        if let Some(editor) = self.mod_hud_editor.as_mut().filter(|e| e.open) {
            editor.finish(false);
        }
    }

    /// Host-owned result, delivered only to the component that requested this draft.
    pub fn take_mod_hud_editor_result(&mut self) -> Option<EditorResult> {
        let editor = self.mod_hud_editor.as_mut()?;
        let result = editor.result.take()?;
        if !editor.open {
            self.mod_hud_editor = None;
        }
        Some(result)
    }

    /// Each open draft row's item icon from `icon`; `None` while the editor is closed.
    pub fn mod_hud_editor_icons(
        &self,
        icon: impl Fn(&str, u32) -> Option<IconRef>,
    ) -> Option<RowIcons> {
        let editor = self.mod_hud_editor.as_ref().filter(|e| e.open)?;
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
            return;
        };
        let Some(editor) = self.mod_hud_editor.as_mut().filter(|e| e.open) else {
            return;
        };
        let px = paint_frame.metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        let content = paint_frame.content;
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
                    tracing::warn!(%error, "HUD editor template rejected");
                    return;
                }
            }
        }
        let mut data = editor.data();
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
        let mut items = ItemRenderer {
            icons: &icons,
            id_aux: &[],
        };
        let mut renderers: [&mut dyn CustomRenderer; 1] = [&mut items];
        let art = ScreenArt {
            view: Some(&editor.view),
            ..Default::default()
        };
        let result = renderer.draw(
            art,
            &mut paint_frame.with_renderers(&mut renderers),
            |env, root| {
                editor.screen.render_shared_with(
                    SCREEN,
                    editor.catalog.as_ref().expect("installed"),
                    &Context::default(),
                    Arc::clone(&editor.data),
                    (root, px, language),
                    env,
                    &editor.view,
                )
            },
        );
        match result {
            Ok(Some(frame)) => editor.frame = Some(frame),
            result => {
                paint_frame.nodes.truncate(rollback.0);
                *paint_frame.next = rollback.1;
                editor.finish(false);
                if let Err(error) = result {
                    tracing::warn!(%error, "HUD editor disabled after rendering failure");
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
            crate::mod_panel::surface::bind(surface, &mut data);
            crate::mod_panel::surface::viewport(&mut data, self.viewport);
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
            drag: editor.drag.as_ref().map(|drag| drag.guides),
        })
    }
}
