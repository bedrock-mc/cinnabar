//! One preview session: the workspace plus a view (screen, size, GUI scale,
//! context flags, mock data), run through the engine's resolve, bind, layout
//! and emit, each stage cached on exactly what it reads so an edit, a data
//! change, a resize or an animation tick reruns only the stages below it.

use std::collections::BTreeMap;
use std::sync::Arc;

use json_ui::{Catalog, CatalogLibrary, Context, DrawNode, LaidOut, LayoutEnv, ResolvedControl};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::diagnose::{self, Diagnostic};
use crate::index::Index;
use crate::mock::MockData;
use crate::paint::{self, PaintInput};
use crate::text::Fonts;
use crate::textures::{TextureCache, Textures};
use crate::workspace::Workspace;

/// What to preview and how.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct View {
    /// `namespace.control`.
    pub reference: String,
    /// Physical window size in pixels.
    pub size: [u32; 2],
    /// Fixed GUI scale; `None` or 0 follows the client's automatic rule.
    pub gui_scale: Option<u8>,
    /// Context variables without `$` (`desktop_screen`, `touch`, ...).
    pub context: BTreeMap<String, Value>,
    pub mock: MockData,
}

impl View {
    pub fn px(&self) -> u32 {
        ui::gui_scale(self.size, self.gui_scale)
    }

    fn context(&self) -> Context {
        self.context
            .iter()
            .fold(Context::empty(), |context, (name, value)| {
                context.with_var(name.trim_start_matches('$'), value.clone())
            })
    }
}

/// A placed control, flattened in document order.
#[derive(Clone, Debug, Serialize)]
pub struct LaidBox {
    pub key: String,
    pub name: String,
    #[serde(rename = "type")]
    pub control_type: Option<String>,
    /// Virtual-pixel `[x, y, w, h]`.
    pub rect: [f64; 4],
    pub visible: bool,
    pub layer: i32,
    pub parent: Option<usize>,
    pub animated: bool,
    /// Child indices from the bound root to this control.
    #[serde(skip)]
    pub path: Vec<usize>,
}

/// One laid-out frame: everything below layout, reusable across paints.
pub struct Frame {
    pub root: [f64; 2],
    pub px: u32,
    pub size: [u32; 2],
    pub boxes: Vec<LaidBox>,
    pub nodes: Vec<DrawNode>,
    pub bound: Arc<ResolvedControl>,
    pub clocks: BTreeMap<String, f64>,
    pub diagnostics: Vec<Diagnostic>,
    /// Texture files the host should supply, as `(layer, path)`.
    pub wanted: Vec<(usize, String)>,
}

struct Resolved {
    key: (u64, String, Context),
    root: Option<Arc<ResolvedControl>>,
    diagnostics: Vec<String>,
}

struct Bound {
    key: (usize, MockData),
    tree: Arc<ResolvedControl>,
    clocks: BTreeMap<String, f64>,
    diagnostics: Vec<String>,
}

#[derive(Default)]
pub struct Session {
    pub workspace: Workspace,
    pub fonts: Fonts,
    textures: TextureCache,
    index: Option<(u64, Arc<Index>)>,
    resolved: Option<Resolved>,
    bound: Option<Bound>,
    frame: Option<(FrameKey, Arc<Frame>)>,
}

#[derive(PartialEq)]
struct FrameKey {
    bound: usize,
    size: [u32; 2],
    px: u32,
    textures: (u64, u64),
    fonts: u64,
}

impl Session {
    pub fn catalog(&mut self) -> Arc<Catalog> {
        let lang = self.workspace.lang();
        self.fonts.set_lang(lang);
        self.workspace.catalog()
    }

    pub fn index(&mut self) -> Arc<Index> {
        let generation = self.workspace.generation();
        if let Some((built, index)) = &self.index
            && *built == generation
        {
            return Arc::clone(index);
        }
        let index = Arc::new(Index::build(&self.workspace));
        self.index = Some((generation, Arc::clone(&index)));
        index
    }

    /// Resolve, bind and lay out `view`, reusing every stage whose inputs held.
    pub fn frame(&mut self, view: &View) -> Arc<Frame> {
        let catalog = self.catalog();
        let generation = self.workspace.generation();
        let base_context = view.context();
        let prepared = view.mock.prepare(&base_context);
        let resolved_key = (generation, view.reference.clone(), prepared.context.clone());
        if self.resolved.as_ref().is_none_or(|r| r.key != resolved_key) {
            let resolution = json_ui::resolve(&catalog, &view.reference, &prepared.context);
            self.resolved = Some(Resolved {
                key: resolved_key,
                root: resolution.control.map(Arc::new),
                diagnostics: resolution.diagnostics,
            });
            self.bound = None;
        }
        let resolved = self.resolved.as_ref().expect("resolved above");
        let Some(root) = resolved.root.clone() else {
            return Arc::new(self.empty_frame(view, &catalog));
        };
        let root_id = Arc::as_ptr(&root) as usize;
        let bound_key = (root_id, view.mock.clone());
        if self.bound.as_ref().is_none_or(|b| b.key != bound_key) {
            let library = CatalogLibrary {
                catalog: &catalog,
                context: &prepared.context,
            };
            let (tree, diagnostics) = json_ui::bind_reporting(&root, &prepared.data, &library);
            self.bound = Some(Bound {
                key: bound_key,
                tree: Arc::new(tree),
                clocks: prepared.clocks,
                diagnostics,
            });
        }
        let bound = self.bound.as_ref().expect("bound above");
        self.textures.sync(&self.workspace);
        let px = view.px();
        let key = FrameKey {
            bound: Arc::as_ptr(&bound.tree) as usize,
            size: view.size,
            px,
            textures: (generation, self.workspace.texture_generation()),
            fonts: self.fonts.revision(),
        };
        if let Some((built, frame)) = &self.frame
            && *built == key
        {
            return Arc::clone(frame);
        }
        let tree = Arc::clone(&bound.tree);
        let clocks = bound.clocks.clone();
        let mut messages = resolved.diagnostics.clone();
        messages.extend(bound.diagnostics.iter().cloned());
        let root_size = [
            f64::from(view.size[0]) / f64::from(px),
            f64::from(view.size[1]) / f64::from(px),
        ];
        let index = self.index();
        let textures = Textures::new(&mut self.workspace, &mut self.textures);
        let measure = self.fonts.measure(px as f32);
        let env = LayoutEnv {
            text: &measure,
            textures: &textures,
        };
        let (laid, _) = json_ui::layout_with(&tree, root_size, &env, &Default::default());
        let nodes = json_ui::emit(&laid, &env);
        let mut boxes = Vec::new();
        flatten(&laid, None, &mut Vec::new(), &mut boxes);
        drop(laid);
        let wanted: Vec<(usize, String)> = textures.wanted.borrow().iter().cloned().collect();
        let missing: Vec<String> = textures.missing.borrow().iter().cloned().collect();
        drop(textures);
        let mut diagnostics = diagnose::collect(&catalog, &index, &messages, &self.workspace);
        diagnostics.extend(diagnose::lint_bindings(&root));
        diagnostics.extend(diagnose::textures(&missing));
        let frame = Arc::new(Frame {
            root: root_size,
            px,
            size: view.size,
            boxes,
            nodes,
            bound: tree,
            clocks,
            diagnostics,
            wanted,
        });
        self.frame = Some((key, Arc::clone(&frame)));
        frame
    }

    fn empty_frame(&mut self, view: &View, catalog: &Catalog) -> Frame {
        let index = self.index();
        let messages = self
            .resolved
            .as_ref()
            .map(|resolved| resolved.diagnostics.clone())
            .unwrap_or_default();
        let px = view.px();
        Frame {
            root: [
                f64::from(view.size[0]) / f64::from(px),
                f64::from(view.size[1]) / f64::from(px),
            ],
            px,
            size: view.size,
            boxes: Vec::new(),
            nodes: Vec::new(),
            bound: Arc::new(ResolvedControl {
                name: String::new(),
                control_type: None,
                base: None,
                unresolved_base: None,
                properties: Default::default(),
                children: Vec::new(),
                factory: None,
            }),
            clocks: BTreeMap::new(),
            diagnostics: diagnose::collect(catalog, &index, &messages, &self.workspace),
            wanted: Vec::new(),
        }
    }

    /// Rasterize `frame` at animation time `now` (seconds).
    pub fn paint(&mut self, frame: &Frame, now: f64) -> Result<Vec<u8>, String> {
        let textures = Textures::new(&mut self.workspace, &mut self.textures);
        let input = PaintInput {
            nodes: &frame.nodes,
            px: frame.px as f32,
            size: frame.size,
            now,
            clocks: &frame.clocks,
        };
        paint::paint(&input, &self.fonts, &textures)
    }
}

fn flatten(node: &LaidOut, parent: Option<usize>, path: &mut Vec<usize>, out: &mut Vec<LaidBox>) {
    let index = out.len();
    out.push(LaidBox {
        key: node.key.clone(),
        name: node.control.name.clone(),
        control_type: node.control.control_type.clone(),
        rect: [node.rect.x, node.rect.y, node.rect.w, node.rect.h],
        visible: node.visible,
        layer: node.layer,
        parent,
        animated: node.anim.is_some(),
        path: path.clone(),
    });
    for child in &node.children {
        let position = node
            .control
            .children
            .iter()
            .position(|candidate| std::ptr::eq(candidate, child.control));
        path.push(position.unwrap_or(usize::MAX));
        flatten(child, Some(index), path, out);
        path.pop();
    }
}

/// The node of `tree` at child-index `path`.
pub fn node_at<'a>(tree: &'a ResolvedControl, path: &[usize]) -> Option<&'a ResolvedControl> {
    path.iter()
        .try_fold(tree, |node, &index| node.children.get(index))
}

#[cfg(test)]
mod review_tests {
    use super::*;
    #[test]
    fn review_loading_a_font_invalidates_the_cached_layout_frame() {
        let mut session = Session::default();
        let layer = session.workspace.add_layer("pack");
        session.workspace.add_files(layer, vec![("ui/_ui_defs.json".into(), br#"{"ui_defs":["ui/test.json"]}"#.to_vec()), ("ui/_global_variables.json".into(), b"{}".to_vec()), ("ui/test.json".into(), br#"{"namespace":"test","main":{"type":"label","text":"A","size":["default","default"]}}"#.to_vec())], vec![]);
        let view = View {
            reference: "test.main".into(),
            size: [320, 240],
            ..Default::default()
        };
        let before = session.frame(&view);
        assert!(!before.boxes.is_empty(), "fixture must resolve");
        assert!(
            Arc::ptr_eq(&before, &session.frame(&view)),
            "fixture must hit the layout cache"
        );
        let manifest = assets::canonical_source_manifest_sha256(include_bytes!(
            "../../../assets/cinnangles-sans-source.json"
        ));
        let page = assets::FontTexturePage {
            source_path: "font/test.png".into(),
            source_bytes: 1,
            source_sha256: [1; 32],
            pixels_sha256: [
                173, 149, 19, 27, 192, 183, 153, 192, 177, 175, 71, 127, 177, 79, 207, 38, 166,
                169, 247, 96, 121, 228, 139, 240, 144, 172, 183, 232, 54, 123, 253, 14,
            ],
            width: 1,
            height: 1,
            pixels: assets::FontPixels::Rgba8(vec![255; 4].into_boxed_slice()),
        };
        let glyph = assets::GlyphMetrics {
            codepoint: 'A',
            page: 0,
            uv: [0, 0, 1, 1],
            bearing: [0, -1],
            advance_64: 512,
        };
        let bytes = assets::encode_font_catalog(manifest, &[glyph], &[page]).unwrap();
        session.fonts.load(&bytes).unwrap();
        let after = session.frame(&view);
        assert!(!Arc::ptr_eq(&before, &after));
        assert_ne!(before.boxes[0].rect, after.boxes[0].rect);
    }
}
