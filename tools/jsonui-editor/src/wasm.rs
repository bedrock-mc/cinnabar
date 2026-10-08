//! Browser bindings: an [`Editor`] the page drives with JSON strings and byte
//! arrays. Files arrive in batches per layer; texture images are announced as
//! pending and supplied when a render asks for them.

use std::sync::Arc;

use serde_json::{Value, json};
use wasm_bindgen::prelude::*;

use crate::api::{self, TreeLimits};
use crate::export;
use crate::scene::{Frame, Session, View};

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn error(message: &str);
}

#[wasm_bindgen]
pub struct Editor {
    session: Session,
    view: View,
    frame: Option<Arc<Frame>>,
    staged: Vec<(String, Vec<u8>)>,
    staged_pending: Vec<String>,
}

fn js_error(message: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&message.to_string())
}

#[wasm_bindgen]
impl Editor {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Editor {
        std::panic::set_hook(Box::new(|info| error(&info.to_string())));
        Editor {
            session: Session::default(),
            view: View {
                size: [1280, 720],
                ..View::default()
            },
            frame: None,
            staged: Vec::new(),
            staged_pending: Vec::new(),
        }
    }

    /// Load the compiled Cinnangles Sans carrier.
    pub fn load_font(&mut self, bytes: &[u8]) -> Result<(), JsValue> {
        self.frame = None;
        self.session.fonts.load(bytes).map_err(js_error)
    }

    pub fn has_font(&self) -> bool {
        self.session.fonts.font().is_some()
    }

    pub fn add_layer(&mut self, name: &str) -> usize {
        self.frame = None;
        self.session.workspace.add_layer(name)
    }

    pub fn remove_layer(&mut self, layer: usize) {
        self.frame = None;
        self.session.workspace.remove_layer(layer);
    }

    /// Queue a file (path relative to the dropped folder) for [`Self::commit_files`].
    pub fn stage_file(&mut self, path: &str, bytes: Vec<u8>) {
        self.staged.push((path.to_owned(), bytes));
    }

    /// Queue a file that exists but whose bytes arrive later via [`Self::supply`].
    pub fn stage_pending(&mut self, path: &str) {
        self.staged_pending.push(path.to_owned());
    }

    pub fn commit_files(&mut self, layer: usize) {
        self.frame = None;
        let files = std::mem::take(&mut self.staged);
        let pending = std::mem::take(&mut self.staged_pending);
        self.session.workspace.add_files(layer, files, pending);
    }

    pub fn add_zip(&mut self, layer: usize, bytes: Vec<u8>) -> Result<(), JsValue> {
        self.frame = None;
        self.session
            .workspace
            .add_archive(layer, bytes)
            .map_err(js_error)
    }

    pub fn supply(&mut self, layer: usize, path: &str, bytes: Vec<u8>) {
        self.session.workspace.supply(layer, path, bytes);
    }

    pub fn edit(&mut self, layer: usize, path: &str, text: &str) {
        self.session.workspace.edit(layer, path, text);
    }

    /// `[{layer, name, files: [{path, edited}]}]` for every layer's ui json.
    pub fn files(&self) -> String {
        let layers: Vec<Value> = self
            .session
            .workspace
            .layers()
            .iter()
            .enumerate()
            .map(|(index, layer)| {
                let files: Vec<Value> = layer
                    .ui_paths()
                    .map(|path| json!({ "path": path, "edited": layer.is_edited(path) }))
                    .collect();
                json!({ "layer": index, "name": layer.name, "scratch": layer.is_scratch(), "files": files })
            })
            .collect();
        Value::Array(layers).to_string()
    }

    /// A new scratch-layer file holding `text` (a starter screen when empty):
    /// `{layer, path}`.
    pub fn new_scratch_file(&mut self, text: &str) -> String {
        self.frame = None;
        let (layer, path) = self.session.workspace.new_scratch_file(text);
        json!({ "layer": layer, "path": path }).to_string()
    }

    /// The control to preview for a file, if it has one.
    pub fn pick_screen(&mut self, layer: usize, path: &str) -> Option<String> {
        api::pick_screen(&mut self.session, layer, path)
    }

    pub fn has_control(&mut self, reference: &str) -> bool {
        api::has_control(&mut self.session, reference)
    }

    pub fn file_text(&self, layer: usize, path: &str) -> Option<String> {
        self.session.workspace.layer(layer)?.text(path)
    }

    pub fn screens(&mut self) -> String {
        json!(api::screens(&mut self.session)).to_string()
    }

    pub fn context_presets(&self) -> String {
        crate::context_presets().to_string()
    }

    /// Replace the view (`{reference, size, gui_scale, context, mock}`).
    pub fn set_view(&mut self, view: &str) -> Result<(), JsValue> {
        let view: View = serde_json::from_str(view).map_err(js_error)?;
        if view.size[0] == 0 || view.size[1] == 0 || view.size[0] > 8192 || view.size[1] > 8192 {
            return Err(js_error("size must be within 1..=8192"));
        }
        self.view = view;
        Ok(())
    }

    /// Resolve, bind and lay out the view; returns boxes, diagnostics and the
    /// `(layer, path)` textures the host should supply before painting again.
    pub fn render(&mut self) -> String {
        let frame = self.session.frame(&self.view);
        let out = json!({
            "width": frame.size[0],
            "height": frame.size[1],
            "gui_scale": frame.px,
            "root": frame.root,
            "boxes": frame.boxes,
            "visible": api::visible_boxes(&frame.boxes),
            "diagnostics": frame.diagnostics,
            "wanted": frame.wanted,
            "animated": frame.boxes.iter().any(|laid| laid.animated),
        });
        self.frame = Some(frame);
        out.to_string()
    }

    /// RGBA8 pixels of the last render at animation time `now` (seconds).
    pub fn paint(&mut self, now: f64) -> Result<Vec<u8>, JsValue> {
        let frame = self.frame.clone().ok_or_else(|| js_error("render first"))?;
        self.session.paint(&frame, now).map_err(js_error)
    }

    /// The box index under a virtual-pixel point, or -1.
    pub fn pick(&self, x: f64, y: f64) -> i32 {
        self.frame
            .as_ref()
            .and_then(|frame| api::pick(frame, [x, y]))
            .map_or(-1, |index| index as i32)
    }

    /// Properties with provenance and the definition of box `index`.
    pub fn inspect(&mut self, index: usize) -> String {
        let Some(frame) = self.frame.clone() else {
            return "null".into();
        };
        let reference = self.view.reference.clone();
        json!(api::inspect(&mut self.session, &reference, &frame, index)).to_string()
    }

    /// The bound tree as JSON (no provenance), for export or debugging.
    pub fn tree(&mut self) -> String {
        let Some(frame) = self.frame.clone() else {
            return "null".into();
        };
        let reference = self.view.reference.clone();
        let limits = TreeLimits {
            max_nodes: 20_000,
            properties: false,
        };
        api::tree_json(&mut self.session, &reference, &frame, limits).to_string()
    }

    /// A syntax check of editor text: `null` or `{offset, line, column, message}`.
    pub fn lint(&self, text: &str) -> String {
        match crate::outline::parse(text) {
            Ok(_) => "null".into(),
            Err(error) => {
                let (line, column) = crate::outline::line_col(text, error.offset);
                json!({ "offset": error.offset, "line": line, "column": column, "message": error.message })
                    .to_string()
            }
        }
    }

    /// A new pack's settings: the given UUIDs, version 1.0.0, the pinned game version.
    pub fn default_pack(&self, name: &str, header_uuid: &str, module_uuid: &str) -> String {
        let uuids = [header_uuid.to_owned(), module_uuid.to_owned()];
        json!(export::PackInfo::new(name, uuids)).to_string()
    }

    /// `pack` one patch version on (`PackInfo` JSON in and out).
    pub fn bump_pack(&self, pack: &str) -> Result<String, JsValue> {
        let pack: export::PackInfo = serde_json::from_str(pack).map_err(js_error)?;
        Ok(json!(pack.bumped()).to_string())
    }

    /// What `request` would ship: `{plan, manifest, warnings}`.
    pub fn export_plan(&mut self, request: &str) -> Result<String, JsValue> {
        let request: export::Request = serde_json::from_str(request).map_err(js_error)?;
        let plan = export::plan(
            &mut self.session.workspace,
            request.mode,
            request.layer,
            request.own_layer,
        );
        let warnings = api::export_warnings(&mut self.session);
        Ok(json!({
            "plan": plan,
            "manifest": request.pack.manifest().map_err(js_error)?,
            "warnings": warnings,
        })
        .to_string())
    }

    /// The packaged export for `request`, with an optional `pack_icon.png`.
    pub fn export_pack(
        &mut self,
        request: &str,
        icon: Option<Vec<u8>>,
    ) -> Result<Vec<u8>, JsValue> {
        let request: export::Request = serde_json::from_str(request).map_err(js_error)?;
        let plan = export::plan(
            &mut self.session.workspace,
            request.mode,
            request.layer,
            request.own_layer,
        );
        export::package(&plan, &request.pack, icon.as_deref(), request.format).map_err(js_error)
    }

    /// The download name for a pack called `name` in `format` (`mcpack`, `zip`, `mcaddon`).
    pub fn pack_file_name(&self, name: &str, format: &str) -> String {
        format!("{}.{format}", export::file_stem(name))
    }

    /// Warnings for a `pack_icon.png`, or an error when it is not a PNG.
    pub fn check_icon(&self, bytes: &[u8]) -> Result<String, JsValue> {
        export::check_icon(bytes)
            .map(|w| json!(w).to_string())
            .map_err(js_error)
    }
}

impl Default for Editor {
    fn default() -> Self {
        Self::new()
    }
}
