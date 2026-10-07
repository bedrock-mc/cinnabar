//! The MCP tools: definitions, argument parsing, and dispatch into the editor
//! core's [`jsonui_editor::api`].

use std::path::{Path, PathBuf};

use jsonui_editor::api::{self, TreeLimits};
use jsonui_editor::mock::MockData;
use jsonui_editor::{Session, View};
use mcp_stdio::text_content;
use serde_json::{Value, json};

/// Files read eagerly from a pack folder; texture images load on demand.
const EAGER_PREFIXES: &[&str] = &["ui/", "texts/"];
const TEXTURE_PREFIX: &str = "textures/";
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
/// Rounds of supplying wanted textures before a render gives up on them.
const TEXTURE_ROUNDS: usize = 4;
/// Largest PNG returned inline as image content.
const MAX_INLINE_PNG: usize = 4 * 1024 * 1024;

pub struct Server {
    session: Session,
    /// Pack roots on disk per layer, for textures supplied on demand.
    roots: Vec<Option<PathBuf>>,
    font_error: Option<String>,
}

impl mcp_stdio::ToolServer for Server {
    fn info(&self) -> (&'static str, &'static str) {
        ("jsonui-mcp", env!("CARGO_PKG_VERSION"))
    }

    fn definitions(&self) -> Value {
        definitions()
    }

    fn call(&mut self, name: &str, arguments: &Value) -> Value {
        Server::call(self, name, arguments)
    }
}

impl Server {
    pub fn new(font: Option<String>) -> Self {
        let mut server = Self {
            session: Session::default(),
            roots: Vec::new(),
            font_error: None,
        };
        if let Some(path) = font {
            server.font_error = server.load_font(Path::new(&path)).err();
        }
        server
    }

    fn load_font(&mut self, path: &Path) -> Result<(), String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        self.session.fonts.load(&bytes)
    }

    /// Run tool `name`, wrapping its outcome as MCP content.
    pub fn call(&mut self, name: &str, arguments: &Value) -> Value {
        let outcome = match name {
            "load_packs" => self.load_packs(arguments),
            "list_screens" => Ok(self.list_screens(arguments)),
            "resolve" => self.resolve(arguments),
            "validate" => Ok(self.validate(arguments)),
            "layout" => self.layout(arguments),
            "render_png" => return self.render_png(arguments),
            "edit_file" => self.edit_file(arguments),
            "export_pack" => self.export_pack(arguments),
            _ => Err(format!("unknown tool `{name}`")),
        };
        match outcome {
            Ok(value) => text_content(&value, false),
            Err(message) => text_content(&json!({ "error": message }), true),
        }
    }

    fn load_packs(&mut self, arguments: &Value) -> Result<Value, String> {
        let paths: Vec<String> = arguments
            .get("paths")
            .and_then(Value::as_array)
            .ok_or("`paths` must be an array of pack folders or zips, bottom first")?
            .iter()
            .filter_map(|path| path.as_str().map(str::to_owned))
            .collect();
        if let Some(font) = arguments.get("font").and_then(Value::as_str) {
            self.load_font(Path::new(font))?;
            self.font_error = None;
        }
        let mut session = Session::default();
        std::mem::swap(&mut session.fonts, &mut self.session.fonts);
        self.session = session;
        self.roots.clear();
        let mut layers = Vec::new();
        for path in &paths {
            let path = PathBuf::from(path);
            let name = path.file_name().map_or_else(
                || path.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
            let layer = self.session.workspace.add_layer(&name);
            if path.is_dir() {
                let root = pack_root(&path);
                let (files, pending) = read_pack(&root)?;
                self.session.workspace.add_files(layer, files, pending);
                self.roots.push(Some(root));
            } else {
                let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                self.session.workspace.add_archive(layer, bytes)?;
                self.roots.push(None);
            }
            let ui_files = self.session.workspace.layers()[layer].ui_paths().count();
            layers.push(json!({ "layer": layer, "name": name, "ui_files": ui_files }));
        }
        let catalog = self.session.catalog();
        Ok(json!({
            "layers": layers,
            "namespaces": catalog.namespace_count(),
            "load_diagnostics": catalog.diagnostics().len(),
            "font": self.font_status(),
        }))
    }

    fn font_status(&self) -> Value {
        match (&self.font_error, self.session.fonts.font().is_some()) {
            (Some(error), _) => json!(format!("font failed to load: {error}")),
            (None, true) => json!("loaded"),
            (None, false) => json!("none: text measures with a fixed-advance fallback"),
        }
    }

    fn list_screens(&mut self, arguments: &Value) -> Value {
        let all = arguments.get("include_all").and_then(Value::as_bool) == Some(true);
        let filter = arguments
            .get("filter")
            .and_then(Value::as_str)
            .unwrap_or("");
        let screens: Vec<_> = api::screens(&mut self.session)
            .into_iter()
            .filter(|entry| (all || entry.screen) && entry.reference.contains(filter))
            .collect();
        json!({ "count": screens.len(), "screens": screens })
    }

    fn view(&self, arguments: &Value) -> Result<View, String> {
        let reference = arguments
            .get("reference")
            .and_then(Value::as_str)
            .ok_or("`reference` (namespace.control) is required")?;
        let size = match arguments.get("size") {
            Some(value) => serde_json::from_value::<[u32; 2]>(value.clone())
                .map_err(|_| "`size` must be [width, height] in physical pixels")?,
            None => [1920, 1080],
        };
        if size[0] == 0 || size[1] == 0 || size[0] > 8192 || size[1] > 8192 {
            return Err("`size` must be within 1..=8192 on each axis".into());
        }
        let gui_scale = arguments
            .get("scale")
            .and_then(Value::as_u64)
            .map(|scale| scale.clamp(0, 8) as u8);
        let mock = match arguments.get("mock_data") {
            Some(value) => serde_json::from_value::<MockData>(value.clone())
                .map_err(|e| format!("`mock_data`: {e}"))?,
            None => MockData::default(),
        };
        Ok(View {
            reference: reference.to_owned(),
            size,
            gui_scale,
            context: context_argument(arguments.get("context"))?,
            mock,
        })
    }

    /// Lay out `view`, supplying textures from disk until none are wanted.
    fn frame(&mut self, view: &View) -> std::sync::Arc<jsonui_editor::Frame> {
        let mut frame = self.session.frame(view);
        for _ in 0..TEXTURE_ROUNDS {
            if frame.wanted.is_empty() {
                break;
            }
            for (layer, path) in &frame.wanted {
                let bytes = self
                    .roots
                    .get(*layer)
                    .and_then(Option::as_ref)
                    .and_then(|root| std::fs::read(root.join(path)).ok())
                    .unwrap_or_default();
                self.session.workspace.supply(*layer, path, bytes);
            }
            frame = self.session.frame(view);
        }
        frame
    }

    fn resolve(&mut self, arguments: &Value) -> Result<Value, String> {
        let view = self.view(arguments)?;
        let frame = self.frame(&view);
        if frame.boxes.is_empty() {
            return Ok(
                json!({ "error": "reference did not resolve", "diagnostics": frame.diagnostics }),
            );
        }
        let limits = TreeLimits {
            max_nodes: arguments
                .get("max_nodes")
                .and_then(Value::as_u64)
                .map_or(400, |n| n as usize),
            properties: arguments.get("properties").and_then(Value::as_bool) != Some(false),
        };
        let tree = api::tree_json(&mut self.session, &view.reference, &frame, limits);
        Ok(json!({ "tree": tree, "diagnostics": frame.diagnostics }))
    }

    fn validate(&mut self, arguments: &Value) -> Value {
        let files: Vec<String> = arguments
            .get("files")
            .and_then(Value::as_array)
            .map(|files| {
                files
                    .iter()
                    .filter_map(|f| f.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        let context = context_argument(arguments.get("context"))
            .unwrap_or_default()
            .into_iter()
            .fold(json_ui::Context::empty(), |context, (name, value)| {
                context.with_var(&name, value)
            });
        let diagnostics = api::validate(&mut self.session, &files, &context);
        json!({ "count": diagnostics.len(), "diagnostics": diagnostics })
    }

    fn layout(&mut self, arguments: &Value) -> Result<Value, String> {
        let view = self.view(arguments)?;
        let frame = self.frame(&view);
        let visible_only = arguments.get("visible_only").and_then(Value::as_bool) != Some(false);
        let visible = api::visible_boxes(&frame.boxes);
        let boxes: Vec<Value> = frame
            .boxes
            .iter()
            .zip(&visible)
            .filter(|(_, shown)| !visible_only || **shown)
            .map(|(laid, shown)| {
                json!({
                    "key": laid.key,
                    "type": laid.control_type,
                    "rect": laid.rect,
                    "visible": shown,
                    "layer": laid.layer,
                })
            })
            .collect();
        Ok(json!({
            "root": frame.root,
            "gui_scale": frame.px,
            "boxes": boxes,
            "diagnostics": frame.diagnostics,
        }))
    }

    fn render_png(&mut self, arguments: &Value) -> Value {
        match self.render(arguments) {
            Ok((summary, png)) => {
                let mut content = vec![json!({ "type": "text", "text": summary.to_string() })];
                if arguments.get("inline").and_then(Value::as_bool) != Some(false)
                    && png.len() <= MAX_INLINE_PNG
                {
                    content.push(mcp_stdio::png_content(&png));
                }
                json!({ "content": content, "isError": false })
            }
            Err(message) => text_content(&json!({ "error": message }), true),
        }
    }

    fn render(&mut self, arguments: &Value) -> Result<(Value, Vec<u8>), String> {
        let view = self.view(arguments)?;
        let frame = self.frame(&view);
        let now = arguments.get("time").and_then(Value::as_f64).unwrap_or(0.0);
        let rgba = self.session.paint(&frame, now)?;
        let png = encode_png(&rgba, frame.size)?;
        let out = match arguments.get("out").and_then(Value::as_str) {
            Some(path) => PathBuf::from(path),
            None => {
                let dir = std::env::temp_dir().join("jsonui-mcp");
                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                dir.join(format!(
                    "{}-{}x{}.png",
                    view.reference, frame.size[0], frame.size[1]
                ))
            }
        };
        std::fs::write(&out, &png).map_err(|e| format!("{}: {e}", out.display()))?;
        let summary = json!({
            "path": out.display().to_string(),
            "size": frame.size,
            "gui_scale": frame.px,
            "font": self.font_status(),
            "diagnostics": frame.diagnostics.len(),
        });
        Ok((summary, png))
    }
}

impl Server {
    fn edit_file(&mut self, arguments: &Value) -> Result<Value, String> {
        let path = arguments
            .get("path")
            .and_then(Value::as_str)
            .ok_or("`path` is required")?;
        let text = arguments
            .get("text")
            .and_then(Value::as_str)
            .ok_or("`text` is required")?;
        let (layer, path) = match arguments.get("layer").and_then(Value::as_u64) {
            _ if arguments.get("scratch").and_then(Value::as_bool) == Some(true) => {
                self.session.workspace.new_scratch_file(text)
            }
            Some(layer) if (layer as usize) < self.session.workspace.layers().len() => {
                self.session.workspace.edit(layer as usize, path, text);
                (layer as usize, path.to_owned())
            }
            Some(layer) => return Err(format!("no layer {layer}")),
            None => {
                let layers = self.session.workspace.layers();
                let holder = (0..layers.len())
                    .rev()
                    .find(|l| layers[*l].file(path).is_some());
                let layer =
                    holder.ok_or("no layer has that file; pass `layer` or `scratch: true`")?;
                self.session.workspace.edit(layer, path, text);
                (layer, path.to_owned())
            }
        };
        let syntax = jsonui_editor::outline::parse(text).err().map(|e| e.message);
        Ok(json!({ "layer": layer, "path": path, "syntax_error": syntax }))
    }

    fn export_pack(&mut self, arguments: &Value) -> Result<Value, String> {
        use jsonui_editor::export::{self, Format, Mode, PackInfo};
        let out = PathBuf::from(
            arguments
                .get("out")
                .and_then(Value::as_str)
                .ok_or("`out` is required")?,
        );
        let mode: Mode =
            serde_json::from_value(arguments.get("mode").cloned().unwrap_or(json!("overlay")))
                .map_err(|_| "`mode` is overlay, changed or full")?;
        let format: Format = match arguments.get("format") {
            Some(format) => serde_json::from_value(format.clone())
                .map_err(|_| "`format` is mcpack, zip or mcaddon")?,
            None => match out.extension().and_then(|e| e.to_str()) {
                Some("zip") => Format::Zip,
                Some("mcaddon") => Format::Mcaddon,
                _ => Format::Mcpack,
            },
        };
        // Re-exporting over an earlier export keeps its UUIDs and bumps its version.
        let previous = std::fs::read(&out)
            .ok()
            .and_then(|bytes| export::read_pack_info(&bytes));
        let fresh = || {
            [
                uuid::Uuid::new_v4().to_string(),
                uuid::Uuid::new_v4().to_string(),
            ]
        };
        let name = arguments.get("name").and_then(Value::as_str);
        let mut pack = match &previous {
            Some(previous) => previous.bumped(),
            None => PackInfo::new(name.unwrap_or("JSON-UI pack"), fresh()),
        };
        if let Some(name) = name {
            pack.name = name.to_owned();
        }
        if let Some(description) = arguments.get("description").and_then(Value::as_str) {
            pack.description = description.to_owned();
        }
        for (key, field) in [
            ("version", &mut pack.version),
            ("min_engine_version", &mut pack.min_engine_version),
        ] {
            if let Some(value) = arguments.get(key) {
                *field = serde_json::from_value(value.clone())
                    .map_err(|_| format!("`{key}` is [major, minor, patch]"))?;
            }
        }
        let icon = match arguments.get("icon").and_then(Value::as_str) {
            Some(path) => Some(std::fs::read(path).map_err(|e| format!("{path}: {e}"))?),
            None => None,
        };
        let layer = arguments
            .get("layer")
            .and_then(Value::as_u64)
            .map(|l| l as usize);
        let own = arguments.get("own_layer").and_then(Value::as_bool) == Some(true);
        let plan = export::plan(&mut self.session.workspace, mode, layer, own);
        let warnings = api::export_warnings(&mut self.session);
        let mut notes = plan.notes.clone();
        if let Some(icon) = &icon {
            notes.extend(export::check_icon(icon)?);
        }
        let bytes = export::package(&plan, &pack, icon.as_deref(), format)?;
        std::fs::write(&out, &bytes).map_err(|e| format!("{}: {e}", out.display()))?;
        Ok(json!({
            "path": out.display().to_string(),
            "pack": pack,
            "files": plan.files.keys().collect::<Vec<_>>(),
            "skipped": plan.skipped.len(),
            "notes": notes,
            "warnings": warnings,
        }))
    }
}

/// A context object, or a preset name from [`jsonui_editor::context_presets`].
fn context_argument(
    value: Option<&Value>,
) -> Result<std::collections::BTreeMap<String, Value>, String> {
    let presets = jsonui_editor::context_presets();
    let value = match value {
        None => presets
            .get("Desktop (Windows)")
            .cloned()
            .unwrap_or_default(),
        Some(Value::String(name)) => presets
            .get(name)
            .cloned()
            .ok_or_else(|| format!("unknown context preset `{name}`"))?,
        Some(other) => other.clone(),
    };
    serde_json::from_value(value).map_err(|_| "`context` must be an object or a preset name".into())
}

/// The folder holding the pack: `path` itself, else the shallowest descendant
/// with a `manifest.json` or `ui/` (two levels deep at most).
fn pack_root(path: &Path) -> PathBuf {
    let is_pack = |dir: &Path| dir.join("manifest.json").is_file() || dir.join("ui").is_dir();
    if is_pack(path) {
        return path.to_path_buf();
    }
    let children = |dir: &Path| -> Vec<PathBuf> {
        std::fs::read_dir(dir)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.is_dir())
                    .collect()
            })
            .unwrap_or_default()
    };
    for child in children(path) {
        if is_pack(&child) {
            return child;
        }
        if let Some(found) = children(&child).into_iter().find(|p| is_pack(p)) {
            return found;
        }
    }
    path.to_path_buf()
}

type PackFiles = (Vec<(String, Vec<u8>)>, Vec<String>);

fn read_pack(root: &Path) -> Result<PackFiles, String> {
    let mut files = Vec::new();
    let mut pending = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(relative) = path.strip_prefix(root) else {
                continue;
            };
            let relative = relative.to_string_lossy().replace('\\', "/");
            if path.is_dir() {
                if EAGER_PREFIXES.iter().chain([&TEXTURE_PREFIX]).any(|p| {
                    let folder = format!("{relative}/");
                    p.starts_with(&folder) || folder.starts_with(p)
                }) {
                    stack.push(path);
                }
                continue;
            }
            let eager = EAGER_PREFIXES.iter().any(|p| relative.starts_with(p))
                || (relative.starts_with(TEXTURE_PREFIX) && relative.ends_with(".json"));
            if eager {
                if entry.metadata().map_or(0, |m| m.len()) <= MAX_FILE_BYTES {
                    let bytes =
                        std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                    files.push((relative, bytes));
                }
            } else if relative.starts_with(TEXTURE_PREFIX) {
                pending.push(relative);
            }
        }
    }
    Ok((files, pending))
}

fn encode_png(rgba: &[u8], size: [u32; 2]) -> Result<Vec<u8>, String> {
    use image::ImageEncoder;
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(rgba, size[0], size[1], image::ExtendedColorType::Rgba8)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

/// Tool names, descriptions and input schemas.
pub fn definitions() -> Value {
    let context = json!({
        "description": "Context variables without `$` (e.g. {\"touch\": true}) or a preset name: \"Desktop (Windows)\" (default), \"Desktop (macOS)\", \"Pocket (touch)\", \"Console\", \"Empty\".",
        "type": ["object", "string"],
    });
    let mock = json!({
        "description": "Mock screen data: {globals: {\"#title_text\": \"..\"}, collections: {name: [{role, \"#value\": ..}]}, factories, grid_dimensions, factory_id, strict, radio, form: {type: action|modal|custom, ..}, hud: {title, sidebar: {title, rows: [[name, score]]}, boss_bars, chat}}.",
        "type": "object",
    });
    let reference =
        json!({ "type": "string", "description": "namespace.control, e.g. start.start_screen" });
    let size = json!({ "type": "array", "items": { "type": "integer" }, "description": "[width, height] physical pixels (default [1920, 1080])" });
    let scale = json!({ "type": "integer", "description": "GUI scale 1-8; omit or 0 for the client's automatic rule" });
    json!([
        {
            "name": "load_packs",
            "description": "Load resource packs (folders or .zip), bottom first: usually the vanilla pack, then server packs. Replaces what was loaded.",
            "inputSchema": { "type": "object", "properties": {
                "paths": { "type": "array", "items": { "type": "string" } },
                "font": { "type": "string", "description": "Compiled Cinnangles Sans carrier (.mcbefont) for exact text metrics" }
            }, "required": ["paths"] }
        },
        {
            "name": "list_screens",
            "description": "List screens (controls whose type resolves to `screen`), or every top-level control with include_all.",
            "inputSchema": { "type": "object", "properties": {
                "include_all": { "type": "boolean" }, "filter": { "type": "string" }
            } }
        },
        {
            "name": "resolve",
            "description": "Resolve and bind a control; returns its tree with each property's source file, layer and `$var` origin, plus diagnostics.",
            "inputSchema": { "type": "object", "properties": {
                "reference": reference, "context": context, "mock_data": mock,
                "max_nodes": { "type": "integer", "description": "Node budget (default 400)" },
                "properties": { "type": "boolean", "description": "Include properties with provenance (default true)" }
            }, "required": ["reference"] }
        },
        {
            "name": "validate",
            "description": "Diagnostics for pack files (all when `files` is empty): syntax, load, unresolved references and bases, undecidable `ignored`, bad bindings.",
            "inputSchema": { "type": "object", "properties": {
                "files": { "type": "array", "items": { "type": "string" }, "description": "Pack-relative paths like ui/hud_screen.json" },
                "context": context
            } }
        },
        {
            "name": "layout",
            "description": "Lay out a control at a window size and GUI scale; returns every control's virtual-pixel box.",
            "inputSchema": { "type": "object", "properties": {
                "reference": reference, "size": size, "scale": scale, "context": context, "mock_data": mock,
                "visible_only": { "type": "boolean", "description": "Default true" }
            }, "required": ["reference"] }
        },
        {
            "name": "render_png",
            "description": "Render a control to a PNG file exactly as the engine lays it out; returns the path and the image.",
            "inputSchema": { "type": "object", "properties": {
                "reference": reference, "size": size, "scale": scale, "context": context, "mock_data": mock,
                "time": { "type": "number", "description": "Animation time in seconds (default 0)" },
                "out": { "type": "string", "description": "Output path (default: a temp file)" },
                "inline": { "type": "boolean", "description": "Also return the PNG as image content (default true)" }
            }, "required": ["reference"] }
        },
        {
            "name": "edit_file",
            "description": "Replace a ui/*.json file's text in memory (the topmost layer holding it, `layer`, or a new scratch-layer file with `scratch: true`); later previews and exports see the edit.",
            "inputSchema": { "type": "object", "properties": {
                "path": { "type": "string" }, "text": { "type": "string" },
                "layer": { "type": "integer" }, "scratch": { "type": "boolean" }
            }, "required": ["path", "text"] }
        },
        {
            "name": "export_pack",
            "description": "Export the edits as a resource pack: `overlay` (only changed controls, layered over the unedited packs), `changed` (edited files in full) or `full` (one layer; unedited files only with own_layer). Re-exporting to the same path keeps the pack's UUIDs and bumps its version.",
            "inputSchema": { "type": "object", "properties": {
                "out": { "type": "string", "description": "Output path (.mcpack, .zip or .mcaddon)" },
                "mode": { "type": "string", "enum": ["overlay", "changed", "full"] },
                "format": { "type": "string", "enum": ["mcpack", "zip", "mcaddon"] },
                "name": { "type": "string" }, "description": { "type": "string" },
                "version": { "type": "array", "items": { "type": "integer" } },
                "min_engine_version": { "type": "array", "items": { "type": "integer" } },
                "icon": { "type": "string", "description": "pack_icon.png path" },
                "layer": { "type": "integer" }, "own_layer": { "type": "boolean" }
            }, "required": ["out"] }
        }
    ])
}
