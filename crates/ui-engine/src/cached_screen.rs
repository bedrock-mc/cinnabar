//! A screen bound and laid out once per model: its resolved tree is kept per catalog and its
//! layout until the data, view, viewport, scale, language or texture metadata change.

use std::sync::Arc;

use json_ui::{
    BindState, CachedLibrary, Catalog, CatalogLibrary, Context, DataSource, FormRender,
    ResolveCache, ResolvedControl, ViewState, bind_incremental, rebind, render_bound_cached,
    resolve,
};

/// One screen's resolved tree per catalog and its last layout per model.
#[derive(Default)]
pub struct CachedScreen {
    resolved: Option<ResolvedScreen>,
    /// Factory and grid resolutions for the resolved catalog, kept across binds.
    library: ResolveCache,
    /// The screen's live bindings across data refreshes.
    binding: BindState,
    laid: Option<Laid>,
    measures: json_ui::MeasureCache,
    /// Bind+layout passes run, for cache tests and profiling.
    pub passes: usize,
    /// Screens resolved on the frame rather than found prepared.
    #[cfg(any(test, feature = "test-support"))]
    pub resolves: usize,
}

struct ResolvedScreen {
    reference: String,
    catalog: Arc<Catalog>,
    context: Context,
    tree: Option<Arc<ResolvedControl>>,
}

struct Laid {
    reference: String,
    catalog: Arc<Catalog>,
    data: Arc<DataSource>,
    view: ViewState,
    root: [f64; 2],
    px: f32,
    language: [usize; 3],
    texture_metadata: u64,
    render: FormRender,
}

impl CachedScreen {
    /// Texture metadata participates in layout, including tiled sprite sizes.
    pub fn invalidate_textures(&mut self) {
        self.laid = None;
        self.measures = json_ui::MeasureCache::default();
    }

    /// Whether the bound HUD has content for an extension to accompany.
    pub fn has_visible_content(&self) -> bool {
        self.laid.as_ref().is_some_and(|laid| {
            laid.render
                .nodes
                .iter()
                .any(|node| node.alpha > 0.0 && node.shown(&laid.view))
        })
    }

    /// The laid-out screen for `data`, rebinding only when an input changed.
    pub fn render(
        &mut self,
        reference: &str,
        catalog: &Arc<Catalog>,
        context: &Context,
        data: Arc<DataSource>,
        at: ([f64; 2], f32, [usize; 3]),
        env: &json_ui::LayoutEnv,
    ) -> Option<&FormRender> {
        self.render_shared_with(
            reference,
            catalog,
            context,
            data,
            at,
            env,
            &ViewState::default(),
        )
    }

    /// [`Self::render`] under the caller's pointer, focus and scroll state.
    #[allow(clippy::too_many_arguments)]
    pub fn render_with(
        &mut self,
        reference: &str,
        catalog: &Arc<Catalog>,
        context: &Context,
        data: DataSource,
        (root, px, language): ([f64; 2], f32, [usize; 3]),
        env: &json_ui::LayoutEnv,
        view: &ViewState,
    ) -> Option<&FormRender> {
        self.render_shared_with(
            reference,
            catalog,
            context,
            Arc::new(data),
            (root, px, language),
            env,
            view,
        )
    }

    /// Reuse a controller revision without cloning or comparing all its bags.
    #[allow(clippy::too_many_arguments)]
    pub fn render_shared_with(
        &mut self,
        reference: &str,
        catalog: &Arc<Catalog>,
        context: &Context,
        data: Arc<DataSource>,
        (root, px, language): ([f64; 2], f32, [usize; 3]),
        env: &json_ui::LayoutEnv,
        view: &ViewState,
    ) -> Option<&FormRender> {
        let texture_metadata = env.textures.metadata_revision();
        let fresh = self.laid.as_ref().is_some_and(|laid| {
            !self.binding.has_pending_visibility()
                && laid.reference == reference
                && Arc::ptr_eq(&laid.catalog, catalog)
                && laid.root == root
                && laid.px == px
                && laid.language == language
                && laid.texture_metadata == texture_metadata
                && self
                    .resolved
                    .as_ref()
                    .is_some_and(|resolved| resolved.context == *context)
                && (Arc::ptr_eq(&laid.data, &data) || laid.data == data)
                && laid.view == *view
        });
        if !fresh {
            let current = self.resolved.as_ref().is_some_and(|resolved| {
                resolved.reference == reference
                    && Arc::ptr_eq(&resolved.catalog, catalog)
                    && resolved.context == *context
            });
            if !current {
                let found = crate::prepared::find(catalog, reference, context);
                let tree = found.unwrap_or_else(|| {
                    #[cfg(any(test, feature = "test-support"))]
                    {
                        self.resolves += 1;
                    }
                    resolve(catalog, reference, context).control.map(Arc::new)
                });
                self.resolved = Some(ResolvedScreen {
                    reference: reference.to_owned(),
                    catalog: Arc::clone(catalog),
                    context: context.clone(),
                    tree,
                });
                self.library = ResolveCache::default();
                self.binding = BindState::new();
            }
            let tree = self.resolved.as_ref()?.tree.as_ref()?;
            let library = CachedLibrary {
                library: CatalogLibrary { catalog, context },
                cache: &self.library,
            };
            let previous = self.laid.take().filter(|_| current);
            let same_frame = previous.as_ref().is_some_and(|laid| {
                laid.root == root
                    && laid.px == px
                    && laid.language == language
                    && laid.texture_metadata == texture_metadata
            });
            let bound = match previous {
                Some(laid) => {
                    let mut bound = laid.render.bound;
                    if !same_frame {
                        self.measures = json_ui::MeasureCache::default();
                    }
                    rebind(
                        tree,
                        &data,
                        &library,
                        &mut self.binding,
                        &mut bound,
                        &mut self.measures,
                    );
                    bound
                }
                None => {
                    self.measures = json_ui::MeasureCache::default();
                    bind_incremental(tree, &data, &library, &mut self.binding)
                }
            };
            self.passes += 1;
            self.laid = Some(Laid {
                reference: reference.to_owned(),
                catalog: Arc::clone(catalog),
                render: render_bound_cached(bound, root, env, view, &mut self.measures),
                data,
                view: view.clone(),
                root,
                px,
                language,
                texture_metadata,
            });
        }
        self.laid.as_ref().map(|laid| &laid.render)
    }
}

impl CachedScreen {
    /// The last laid-out draw nodes, in virtual px.
    pub fn nodes(&self) -> &[json_ui::DrawNode] {
        self.laid
            .as_ref()
            .map_or(&[], |laid| laid.render.nodes.as_slice())
    }
}
