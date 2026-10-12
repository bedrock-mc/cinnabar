//! What screen families register with the engine and the frames they paint through.

use std::{collections::BTreeMap, fmt, sync::Arc};

use assets::RuntimeFontCatalog;
use json_ui::{Catalog, CatalogLibrary, Context, ControlLibrary, ControlRef, ResolvedControl};
use serde_json::Value;
use ui::{SafeArea, TextLayoutCache, UiNode, UiRect};

use super::{Canvas, pack_catalog::CatalogPolicy};
use crate::ui_runtime::{forms::EngineFrame, presentation::TextMetrics};

/// A screen family that paints its own state from a narrow borrowed input; adapters project
/// runtime authority into that input and apply the output.
pub(crate) trait Screen {
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
pub(crate) trait CustomRenderer {
    /// `Ok(false)` leaves `node` to the next renderer and then the engine's own.
    fn paint(&mut self, node: &CustomDraw<'_>, canvas: &mut Canvas<'_>)
    -> Result<bool, PaintError>;

    /// Reports every texture this paint may draw, so it is resident first; `drawn` says
    /// whether a renderer name is laid out this frame.
    fn textures<'s>(&'s self, _drawn: &dyn Fn(&str) -> bool, _texture: &mut dyn FnMut(&'s str)) {}
}

/// A laid-out `custom` node.
pub(crate) struct CustomDraw<'a> {
    pub(crate) key: &'a str,
    pub(crate) renderer: &'a str,
    pub(crate) data: &'a BTreeMap<String, Value>,
    /// Logical bounds on whole physical pixels.
    pub(crate) dest: [f32; 4],
    pub(super) opacity: f32,
}

impl CustomDraw<'_> {
    /// `color` faded by the node's animated opacity.
    pub(crate) fn alpha(&self, color: [u8; 4]) -> [u8; 4] {
        let alpha = (f32::from(color[3]) * self.opacity.clamp(0.0, 1.0)).round() as u8;
        [color[0], color[1], color[2], alpha]
    }
}

/// What one paint borrows: painting resources, the viewport, localization, the caller's
/// clock and output, and the custom renderers its screen supplies.
pub(crate) struct PaintFrame<'a> {
    pub(crate) layouts: &'a mut TextLayoutCache,
    pub(crate) font: &'a RuntimeFontCatalog,
    pub(crate) metrics: TextMetrics,
    pub(crate) solid_page: u16,
    pub(crate) safe_area: SafeArea,
    pub(crate) content: [f32; 2],
    pub(crate) translate: &'a dyn Fn(&str) -> Option<Arc<str>>,
    /// The language tables `translate` reads; a change measures text again.
    pub(crate) language: [usize; 3],
    /// Seconds that animations and fades paint at.
    pub(crate) now: f64,
    pub(crate) nodes: &'a mut Vec<UiNode>,
    pub(crate) next: &'a mut u32,
    pub(crate) renderers: &'a mut [&'a mut dyn CustomRenderer],
}

impl PaintFrame<'_> {
    /// This frame painting through `renderers` instead of its own.
    pub(crate) fn with_renderers<'b>(
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
pub(crate) struct ScreenOutput<A> {
    /// Pointer regions in window-logical px, topmost last.
    pub(crate) hits: Vec<(A, UiRect)>,
    /// Keyboard focus order, when the screen captures focus.
    pub(crate) focus: Option<Vec<A>>,
    /// Engine-laid-out regions, scroll extents, edit-text placement and overlay ranges.
    pub(crate) frame: Option<EngineFrame>,
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
pub(crate) type TreeHook = fn(&mut ResolvedControl);

/// Definitions the screen families register: catalog policy, the screens the engine may
/// resolve, and binding hooks.
pub(crate) struct ScreenRegistry {
    context: Context,
    catalog: CatalogPolicy,
    screens: Vec<&'static str>,
    builtin_screens: Vec<&'static str>,
    tree_hooks: Vec<TreeHook>,
}

impl ScreenRegistry {
    /// An empty registry whose screens resolve under `context`.
    pub(crate) fn new(context: Context) -> Self {
        Self {
            context,
            catalog: CatalogPolicy::default(),
            screens: Vec::new(),
            builtin_screens: Vec::new(),
            tree_hooks: Vec::new(),
        }
    }

    pub(crate) fn catalog_mut(&mut self) -> &mut CatalogPolicy {
        &mut self.catalog
    }

    /// Lets the engine resolve, bind and lay out `reference`.
    pub(crate) fn screen(&mut self, reference: &'static str) {
        self.screens.push(reference);
    }

    /// Resolves `reference` against the built-in catalog, never a server pack's.
    pub(crate) fn builtin_screen(&mut self, reference: &'static str) {
        self.builtin_screens.push(reference);
    }

    /// Runs `hook` over every engine screen's resolved tree and factory-created control.
    pub(crate) fn tree_hook(&mut self, hook: TreeHook) {
        self.tree_hooks.push(hook);
    }

    pub(crate) fn context(&self) -> &Context {
        &self.context
    }

    pub(crate) fn catalog(&self) -> &CatalogPolicy {
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
