//! Local panel routes prepared before frame publication, with bounded retained route caches.

use std::sync::Arc;

use json_ui::{Catalog, Context, ResolvedControl};
use ui::mod_panel::{Panel, PanelBundle, Surface};
use ui_engine::cached_screen::CachedScreen;

use super::{SCREEN, template};

/// The loaded component that owns a prepared panel catalog.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PanelUiOwner {
    /// A component's stable index in the environment-loaded set.
    Environment(usize),
    /// The independently registered component, including all of its replacements.
    Registration,
}

impl crate::ModUi {
    /// Replaces or removes one owner's catalog without evicting another active owner's routes.
    pub fn set_prepared_panel_ui(
        &mut self,
        owner: PanelUiOwner,
        prepared: Option<Arc<PreparedPanelUi>>,
    ) {
        if self.prepared_panel_ui.get(&owner).is_some_and(|current| {
            prepared
                .as_ref()
                .is_some_and(|prepared| Arc::ptr_eq(&current.prepared, prepared))
        }) {
            return;
        }
        if self
            .mod_panel
            .as_ref()
            .is_some_and(|panel| panel.owner == owner)
        {
            self.cancel_mod_hud_editor();
            self.mod_panel = None;
        }
        if let Some(prepared) = prepared {
            self.prepared_panel_ui
                .insert(owner, PreparedOwner::new(prepared));
        } else {
            self.prepared_panel_ui.remove(&owner);
        }
    }
}

/// One live owner's immutable routes and lazily rendered screens at their source indexes.
pub(crate) struct PreparedOwner {
    pub(super) prepared: Arc<PreparedPanelUi>,
    pub(super) screens: Vec<Option<CachedScreen>>,
}

impl PreparedOwner {
    /// Derives every cache slot from the owner's immutable route list.
    fn new(prepared: Arc<PreparedPanelUi>) -> Self {
        let screens = prepared.routes.iter().map(|_| None).collect();
        Self { prepared, screens }
    }

    /// Returns the route's rendered cache, starting from its worker-resolved root on first use.
    pub(super) fn screen(&mut self, route: &PreparedRoute) -> &mut CachedScreen {
        self.screens[route.index].get_or_insert_with(|| route.screen())
    }

    /// Reads an existing route cache without allocating or resolving anything.
    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn cached_screen(&self, route: &PreparedRoute) -> Option<&CachedScreen> {
        self.screens[route.index].as_ref()
    }
}

/// Immutable catalogs and resolved roots prepared on the local component loading worker.
pub struct PreparedPanelUi {
    bundle: Arc<PanelBundle>,
    routes: Vec<Arc<PreparedRoute>>,
}

pub(super) struct PreparedRoute {
    /// Position in the owning generation's immutable route list.
    index: usize,
    source: Surface,
    pub(super) catalog: Arc<Catalog>,
    tree: Arc<ResolvedControl>,
}

impl PreparedPanelUi {
    /// Checks and prepares every bounded route once, outside the renderer's frame path.
    pub fn prepare(bundle: Arc<PanelBundle>) -> Result<Arc<Self>, String> {
        bundle.validate()?;
        let mut sources: Vec<&Surface> = bundle.surfaces.values().collect();
        if let Some(surface) = &bundle.panel.surface
            && !sources.iter().any(|source| same_document(source, surface))
        {
            sources.push(surface);
        }
        let mut routes = Vec::with_capacity(sources.len());
        for source in sources {
            let mut panel = bundle.panel.clone();
            panel.surface = Some(source.clone());
            let (catalog, _) = template::catalog(&panel, [0.0; 2], 0, 0, 1, None)?;
            let catalog = Arc::new(catalog);
            let resolution = json_ui::resolve(&catalog, SCREEN, &Context::default());
            if !resolution.diagnostics.is_empty() {
                return Err(resolution.diagnostics.join("; "));
            }
            let tree = Arc::new(resolution.control.ok_or("prepared panel root is missing")?);
            routes.push(Arc::new(PreparedRoute {
                index: routes.len(),
                source: source.clone(),
                catalog,
                tree,
            }));
        }
        Ok(Arc::new(Self { bundle, routes }))
    }

    /// Matches only the exact validated source and its original control action schema.
    pub(super) fn route(&self, panel: &Panel) -> Option<Arc<PreparedRoute>> {
        if !template::same_template(&self.bundle.panel, panel) {
            return None;
        }
        let source = panel.surface.as_ref()?;
        self.routes
            .iter()
            .find(|route| same_document(&route.source, source))
            .cloned()
    }
}

impl PreparedRoute {
    /// Initializes a route without resolving its static document on the frame.
    pub(super) fn screen(&self) -> CachedScreen {
        CachedScreen::from_resolved(
            SCREEN,
            Arc::clone(&self.catalog),
            Context::default(),
            Arc::clone(&self.tree),
        )
    }
}

/// Scalar changes never invalidate a prepared static route.
fn same_document(a: &Surface, b: &Surface) -> bool {
    a.screen == b.screen && a.document == b.document
}
