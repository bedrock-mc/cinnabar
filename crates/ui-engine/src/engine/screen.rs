//! What screen families register with the engine and the frames they paint through.

use std::{collections::BTreeMap, fmt, sync::Arc};

use assets::RuntimeFontCatalog;
use json_ui::{Catalog, CatalogLibrary, Context, ControlLibrary, ControlRef, ResolvedControl};
use serde_json::Value;
use ui::{SafeArea, TextLayoutCache, UiNode, UiRect};

use super::{Canvas, pack_catalog::CatalogPolicy};
use crate::{frame::EngineFrame, text_metrics::TextMetrics};

/// A screen family that paints its own state from a narrow borrowed input; adapters project
/// runtime authority into that input and apply the output.
pub trait Screen {
    /// The observations one paint reads.
    type Input<'a>;
    /// What the screen's hits and focus resolve to.
    type Action;

    /// Records the family's catalog policies, engine screens and binding hooks.
    fn register(registry: &mut ScreenRegistry);

    fn paint(
        &mut self,
        input: Self::Input<'_>,
        frame: &mut PaintFrame<'_>,
    ) -> Result<ScreenOutput<Self::Action>, PaintError>;
}

/// Draws the `custom` controls a screen family owns.
pub trait CustomRenderer {
    /// `Ok(false)` leaves `node` to the next renderer and then the engine's own.
    fn paint(&mut self, node: &CustomDraw<'_>, canvas: &mut Canvas<'_>)
    -> Result<bool, PaintError>;

    /// Reports every texture this paint may draw, so it is resident first; `drawn` says
    /// whether a renderer name is laid out this frame.
    fn textures<'s>(&'s self, _drawn: &dyn Fn(&str) -> bool, _texture: &mut dyn FnMut(&'s str)) {}
}

/// A laid-out `custom` node.
pub struct CustomDraw<'a> {
    pub key: &'a str,
    pub renderer: &'a str,
    pub data: &'a BTreeMap<String, Value>,
    /// Logical bounds on whole physical pixels.
    pub dest: [f32; 4],
    pub(super) opacity: f32,
}

impl CustomDraw<'_> {
    /// `color` faded by the node's animated opacity.
    pub fn alpha(&self, color: [u8; 4]) -> [u8; 4] {
        let alpha = (f32::from(color[3]) * self.opacity.clamp(0.0, 1.0)).round() as u8;
        [color[0], color[1], color[2], alpha]
    }
}

/// A language-key lookup in the active tables; `None` when no table has the key.
pub type Translate<'a> = &'a dyn Fn(&str) -> Option<Arc<str>>;

/// `key`'s translation, else `fallback`.
pub fn translated(translate: Translate<'_>, key: &str, fallback: &str) -> String {
    translate(key).map_or_else(|| fallback.to_owned(), |value| value.to_string())
}

/// What one paint borrows: painting resources, the viewport, localization, the caller's
/// clock and output, and the custom renderers its screen supplies.
pub struct PaintFrame<'a> {
    pub layouts: &'a mut TextLayoutCache,
    pub font: &'a RuntimeFontCatalog,
    pub metrics: TextMetrics,
    pub solid_page: u16,
    pub safe_area: SafeArea,
    pub content: [f32; 2],
    pub translate: &'a dyn Fn(&str) -> Option<Arc<str>>,
    /// The language tables `translate` reads; a change measures text again.
    pub language: [usize; 3],
    /// Seconds that animations and fades paint at.
    pub now: f64,
    pub nodes: &'a mut Vec<UiNode>,
    pub next: &'a mut u32,
    pub renderers: &'a mut [&'a mut dyn CustomRenderer],
}

impl PaintFrame<'_> {
    /// This frame painting through `renderers` instead of its own.
    pub fn with_renderers<'b>(
        &'b mut self,
        renderers: &'b mut [&'b mut dyn CustomRenderer],
    ) -> PaintFrame<'b> {
        PaintFrame {
            layouts: &mut *self.layouts,
            font: self.font,
            metrics: self.metrics,
            solid_page: self.solid_page,
            safe_area: self.safe_area,
            content: self.content,
            translate: self.translate,
            language: self.language,
            now: self.now,
            nodes: &mut *self.nodes,
            next: &mut *self.next,
            renderers,
        }
    }
}

/// What a painted screen hands its adapter.
pub struct ScreenOutput<A> {
    /// Pointer regions in window-logical px, topmost last.
    pub hits: Vec<(A, UiRect)>,
    /// Keyboard focus order, when the screen captures focus.
    pub focus: Option<Vec<A>>,
    /// Engine-laid-out regions, scroll extents, edit-text placement and overlay ranges.
    pub frame: Option<EngineFrame>,
}

/// Why a screen or renderer could not paint.
#[derive(Debug)]
pub enum PaintError {
    InvalidFontTexture,
    Geometry(ui::GeometryError),
    Text(ui::TextError),
    Tree(ui::UiError),
}

impl fmt::Display for PaintError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "UI presentation failed: {self:?}")
    }
}

impl std::error::Error for PaintError {}

/// Rewrites a resolved tree after inheritance and variable substitution.
pub type TreeHook = fn(&mut ResolvedControl);

/// Definitions the screen families register: catalog policy, the screens the engine may
/// resolve, and binding hooks.
pub struct ScreenRegistry {
    context: Context,
    catalog: CatalogPolicy,
    screens: Vec<&'static str>,
    builtin_screens: Vec<&'static str>,
    tree_hooks: Vec<TreeHook>,
    images: Vec<(&'static str, &'static [u8])>,
}

impl ScreenRegistry {
    /// An empty registry whose screens resolve under `context`.
    pub fn new(context: Context) -> Self {
        Self {
            context,
            catalog: CatalogPolicy::default(),
            screens: Vec::new(),
            builtin_screens: Vec::new(),
            tree_hooks: Vec::new(),
            images: Vec::new(),
        }
    }

    pub fn catalog_mut(&mut self) -> &mut CatalogPolicy {
        &mut self.catalog
    }

    /// Lets the engine resolve, bind and lay out `reference`.
    pub fn screen(&mut self, reference: &'static str) {
        self.screens.push(reference);
    }

    /// Resolves `reference` against the built-in catalog, never a server pack's.
    pub fn builtin_screen(&mut self, reference: &'static str) {
        self.builtin_screens.push(reference);
    }

    /// Runs `hook` over every engine screen's resolved tree and factory-created control.
    pub fn tree_hook(&mut self, hook: TreeHook) {
        self.tree_hooks.push(hook);
    }

    /// Ships `bytes` as the image at extensionless pack `path`, below any server pack's.
    pub fn builtin_image(&mut self, path: &'static str, bytes: &'static [u8]) {
        self.images.push((path, bytes));
    }

    pub fn builtin_images(&self) -> &[(&'static str, &'static [u8])] {
        &self.images
    }

    pub fn context(&self) -> &Context {
        &self.context
    }

    pub fn catalog(&self) -> &CatalogPolicy {
        &self.catalog
    }

    pub(super) fn allows(&self, reference: &str) -> bool {
        self.screens.contains(&reference)
    }

    pub(super) fn builtin(&self, reference: &str) -> bool {
        self.builtin_screens.contains(&reference)
    }

    /// `tree` after every registered hook.
    pub(super) fn hooked(&self, tree: Option<ResolvedControl>) -> Option<ResolvedControl> {
        tree.map(|mut tree| {
            for hook in &self.tree_hooks {
                hook(&mut tree);
            }
            tree
        })
    }

    /// The catalog's definitions with every hook applied to what they resolve to.
    pub(super) fn library<'a>(
        &'a self,
        catalog: &'a Catalog,
        context: &'a Context,
    ) -> HookedLibrary<'a> {
        HookedLibrary {
            library: CatalogLibrary { catalog, context },
            registry: self,
        }
    }
}

/// A catalog library whose factory-created controls pass the registered hooks.
pub(super) struct HookedLibrary<'a> {
    library: CatalogLibrary<'a>,
    registry: &'a ScreenRegistry,
}

impl ControlLibrary for HookedLibrary<'_> {
    fn resolve(&self, reference: &ControlRef) -> Option<ResolvedControl> {
        self.registry.hooked(self.library.resolve(reference))
    }

    fn resolve_with(
        &self,
        reference: &ControlRef,
        key: &str,
        vars: &dyn Fn() -> BTreeMap<String, Value>,
    ) -> Option<ResolvedControl> {
        self.registry
            .hooked(self.library.resolve_with(reference, key, vars))
    }
}
