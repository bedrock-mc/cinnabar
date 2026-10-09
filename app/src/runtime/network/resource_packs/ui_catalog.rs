//! Worker preparation of the initial server UI catalog against its exact carrier.

use client_ui::ui_runtime::presentation::ServerUiPack;
use std::sync::{Arc, Mutex, Weak};

#[derive(bevy::prelude::Resource, Clone)]
pub(crate) struct PackUiCatalog(pub Arc<json_ui::Catalog>);

/// Weak, so leaving a server releases its archive stack instead of the cache pinning it.
struct Cached {
    source: Weak<ServerUiPack>,
    base: Weak<json_ui::Catalog>,
    prepared: Weak<ServerUiPack>,
}

#[derive(Default)]
struct CatalogCache(Mutex<Option<Cached>>);

impl CatalogCache {
    fn prepare(
        &self,
        source: &Arc<ServerUiPack>,
        base: &Arc<json_ui::Catalog>,
    ) -> Arc<ServerUiPack> {
        {
            let cached = self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(cached) = cached.as_ref()
                && cached
                    .source
                    .upgrade()
                    .is_some_and(|cached| Arc::ptr_eq(&cached, source))
                && cached
                    .base
                    .upgrade()
                    .is_some_and(|cached| Arc::ptr_eq(&cached, base))
                && let Some(prepared) = cached.prepared.upgrade()
            {
                return prepared;
            }
        }
        let prepared = source.prepare_catalog(base);
        *self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Cached {
            source: Arc::downgrade(source),
            base: Arc::downgrade(base),
            prepared: Arc::downgrade(&prepared),
        });
        prepared
    }
}

/// `source` resolved against `base`, shared with the last caller that prepared the same pair
/// while that result is still alive.
pub(in crate::runtime::network) fn prepare(
    source: &Arc<ServerUiPack>,
    base: &Arc<json_ui::Catalog>,
) -> Arc<ServerUiPack> {
    static CACHE: CatalogCache = CatalogCache(Mutex::new(None));
    CACHE.prepare(source, base)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(name: &str) -> Arc<json_ui::Catalog> {
        let bytes = format!(r#"{{"namespace":"join","{name}":{{"type":"label"}}}}"#);
        Arc::new(
            json_ui::Catalog::from_files([
                ("ui/_global_variables.json", b"{}".as_slice()),
                (
                    "ui/_ui_defs.json",
                    br#"{"ui_defs":["ui/join.json"]}"#.as_slice(),
                ),
                ("ui/join.json", bytes.as_bytes()),
            ])
            .unwrap(),
        )
    }

    fn pack(name: &str) -> Arc<ServerUiPack> {
        Arc::new(ServerUiPack {
            ui_layers: vec![vec![
                (
                    "ui/_ui_defs.json".into(),
                    br#"{"ui_defs":["ui/server.json"]}"#.to_vec(),
                ),
                (
                    "ui/server.json".into(),
                    format!(r#"{{"namespace":"server","{name}":{{"type":"label"}}}}"#).into_bytes(),
                ),
            ]],
            ..Default::default()
        })
    }

    #[test]
    fn initial_catalog_is_ready_before_publication_and_reused_for_unchanged_inputs() {
        let cache = CatalogCache::default();
        let base = base("carrier");
        let pack = pack("label");
        assert!(pack.catalog.is_none());
        let first = cache.prepare(&pack, &base);
        let catalog = first.catalog.as_ref().expect("worker resolves initial UI");
        assert!(catalog.lookup("join", "carrier").is_some());
        assert!(catalog.lookup("server", "label").is_some());
        assert!(Arc::ptr_eq(&first, &cache.prepare(&pack, &base)));
        assert!(
            pack.catalog.is_none(),
            "preparation does not mutate admission"
        );
    }

    #[test]
    fn either_carrier_or_server_replacement_invalidates_the_prepared_catalog() {
        let cache = CatalogCache::default();
        let carrier = base("old");
        let source = pack("old");
        let first = cache.prepare(&source, &carrier);
        let changed = cache.prepare(&source, &base("new"));
        assert!(!Arc::ptr_eq(&first, &changed));
        let catalog = changed.catalog.as_ref().unwrap();
        assert!(catalog.lookup("join", "new").is_some());
        assert!(catalog.lookup("join", "old").is_none());
        let changed = cache.prepare(&pack("new"), &carrier);
        let catalog = changed.catalog.as_ref().unwrap();
        assert!(catalog.lookup("server", "new").is_some());
        assert!(catalog.lookup("server", "old").is_none());
    }

    #[test]
    fn leaving_a_server_releases_its_pack_and_prepared_catalog() {
        let cache = CatalogCache::default();
        let base = base("carrier");
        let source = pack("label");
        let prepared = Arc::downgrade(&cache.prepare(&source, &base));
        assert_eq!(
            Arc::strong_count(&source),
            1,
            "the cache pins the admitted pack"
        );
        assert!(
            prepared.upgrade().is_none(),
            "the cache pins the prepared catalog"
        );
        let released = Arc::downgrade(&source);
        drop(source);
        assert!(released.upgrade().is_none());
    }
}
