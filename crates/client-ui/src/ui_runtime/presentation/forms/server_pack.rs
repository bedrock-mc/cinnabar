//! A joined server's resource-pack UI: each pack's definitions over the carrier catalog, prepared
//! on the reload worker, and the pack stack the engine's atlas reads textures from.

use std::sync::Arc;

use ui_engine::atlas::{IMAGE_EXTENSIONS, preread::PrereadTextures};

mod prepared_settings;
pub use prepared_settings::PreparedScreenSettings;

/// A session's server resource-pack UI: each pack's indexed definitions, lowest
/// precedence first (each layer merges over the ones below), and where its
/// image files read from.
#[derive(Clone, Debug, Default)]
pub struct ServerUiPack {
    pub ui_layers: Vec<Vec<(String, Vec<u8>)>>,
    /// Parsed title resources in ascending pack priority, prepared by the reload worker.
    pub splashes: Arc<[ui_screens::menu_renderers::splashes::SplashSource]>,
    /// Winning texture files read up front (fixture packs).
    pub textures: Vec<(String, Vec<u8>)>,
    /// Item glint pixels decoded on the pack worker, independent of item icon pages.
    pub item_glint: Option<Arc<render_model::UiTexturePage>>,
    /// The session's pack stack, which texture files read from on first draw.
    pub view: Option<resource_pack::LayeredPackView>,
    pub catalog: Option<Arc<json_ui::Catalog>>,
    /// Built-in screens' catalog carrying this pack's filters; absent when it adds none.
    pub builtin_screen_catalog: Option<Arc<json_ui::Catalog>>,
    /// Immutable screen policies prepared alongside the matching catalog.
    pub screen_settings: Option<Arc<PreparedScreenSettings>>,
}

impl assets::RetainedBuffers for ServerUiPack {
    fn visit_buffers(&self, visit: &mut assets::BufferVisitor<'_>) {
        // The stack view's archives are counted with the admission; catalogs, splashes and
        // screen settings are parsed tables, small beside the read sources and pixels.
        let Self {
            ui_layers,
            splashes: _,
            textures,
            item_glint,
            view: _,
            catalog: _,
            builtin_screen_catalog: _,
            screen_settings: _,
        } = self;
        for (_, bytes) in ui_layers.iter().flatten().chain(textures) {
            assets::visit_slice(bytes, visit);
        }
        item_glint.visit_buffers(visit);
    }
}

impl ServerUiPack {
    /// Image extensions used by both pack reload tracking and texture loading.
    pub fn image_extensions() -> &'static [&'static str] {
        &IMAGE_EXTENSIONS
    }

    /// Image files JSON-UI can reference anywhere in an admitted pack.
    pub fn is_image_path(path: &str) -> bool {
        IMAGE_EXTENSIONS
            .iter()
            .any(|extension| path.ends_with(extension))
    }

    /// Resolves pack definitions on the reload worker against the immutable carrier catalog.
    pub fn prepare_catalog(&self, base: &json_ui::Catalog) -> Arc<Self> {
        let mut prepared = self.clone();
        let layered = {
            let _zone = tracing::info_span!("ui.prepare.layer").entered();
            super::screen_registry::registry()
                .catalog()
                .layer(base, &self.ui_layers)
        };
        let catalog = Arc::new(layered.pack);
        // HUD art and screen policies each read only the finished catalog.
        let textures = || {
            let _zone = tracing::info_span!("ui.prepare.hud_textures").entered();
            let textures = super::hud::prepare(&catalog);
            if let Some(view) = &self.view {
                ui_engine::prepared::keep_textures(
                    &catalog,
                    PrereadTextures::read(view, &textures),
                );
            }
        };
        let settings = || {
            let _zone = tracing::info_span!("ui.prepare.screen_settings").entered();
            PreparedScreenSettings::new(catalog.clone(), ui_screens::menu_screens::retail_context())
        };
        #[cfg(not(target_arch = "wasm32"))]
        let ((), settings) = rayon::join(textures, settings);
        #[cfg(target_arch = "wasm32")]
        let ((), settings) = (textures(), settings());
        prepared.screen_settings = Some(Arc::new(settings));
        prepared.catalog = Some(catalog);
        prepared.builtin_screen_catalog = layered.builtin_screens.map(Arc::new);
        Arc::new(prepared)
    }

    pub fn is_empty(&self) -> bool {
        self.ui_layers.iter().all(Vec::is_empty)
            && self.textures.is_empty()
            && self.splashes.is_empty()
    }
}

#[cfg(test)]
#[path = "server_pack/external_paths_tests.rs"]
mod external_paths_tests;

#[cfg(test)]
mod tests;
