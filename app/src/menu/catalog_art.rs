//! Publishes catalog artwork from the host worker into the menu.
use super::MenuRuntime;
use launcher::menu::view::{CatalogServer, MenuServerCard};
use launcher_host::catalog_art::{apply, start};

impl MenuRuntime {
    /// The catalog's cards, with a worker caching their thumbnails beside the catalog file.
    pub(super) fn catalog_cards(&mut self, servers: Vec<CatalogServer>) -> Vec<MenuServerCard> {
        let wanted: Vec<(String, String)> = servers
            .iter()
            .filter(|server| !server.image_url.is_empty())
            .map(|server| (server.address.clone(), server.image_url.clone()))
            .collect();
        self.catalog_art = (!wanted.is_empty())
            .then(|| start(self.catalog.artwork_dir(), wanted))
            .flatten();
        servers.into_iter().map(Into::into).collect()
    }

    /// Fills each card whose thumbnail finished caching.
    pub(super) fn poll_catalog_art(&mut self) {
        let Some(paths) = self.catalog_art.as_ref().and_then(|art| art.poll()) else {
            return;
        };
        self.catalog_art = None;
        apply(&mut self.featured, &paths);
    }
}
