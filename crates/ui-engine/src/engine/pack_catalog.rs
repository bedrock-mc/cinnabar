//! Catalog layering: carrier extensions, the built-in packs under every server pack, and
//! the rules server layers follow, as the screen families register them.
use json_ui::{Catalog, RawControl};

/// Patches a catalog in place.
pub type CatalogPatch = fn(&mut Catalog);

/// A carrier definition restored before the first server layer that authors it, so that
/// layer patches the carrier's definition rather than a built-in pack's.
#[derive(Clone, Copy)]
pub struct CarrierRestore {
    pub authored: fn(&Catalog, &[(String, Vec<u8>)]) -> bool,
    pub restore: fn(&mut Catalog, &Catalog),
}

#[derive(Default)]
pub struct CatalogPolicy {
    extensions: Vec<CatalogPatch>,
    builtin: Vec<CatalogPatch>,
    restores: Vec<CarrierRestore>,
    finishers: Vec<LayerFinisher>,
}

/// Runs after every server layer with the built-in catalog they started from; returns
/// built-in screen controls that gained pack rules.
pub type LayerFinisher = fn(&mut Catalog, &Catalog) -> Vec<RawControl>;

/// A server pack stack over the built-in catalog, and the catalog built-in screens resolve against.
pub struct LayeredCatalogs {
    pub pack: Catalog,
    /// The built-in catalog carrying the pack rules finishers returned; `None` when they returned none.
    pub builtin_screens: Option<Catalog>,
}

impl CatalogPolicy {
    /// Extends the carrier's own catalog, beneath the built-in packs.
    pub fn extend(&mut self, patch: CatalogPatch) {
        self.extensions.push(patch);
    }

    /// Layers a built-in pack over the carrier, beneath every server pack.
    pub fn layer_builtin(&mut self, patch: CatalogPatch) {
        self.builtin.push(patch);
    }

    pub fn restore_before(&mut self, restore: CarrierRestore) {
        self.restores.push(restore);
    }

    /// Runs `finish` after every server layer, with the built-in catalog they started from.
    pub fn after_layers(&mut self, finish: LayerFinisher) {
        self.finishers.push(finish);
    }

    pub(super) fn extended(&self, catalog: &mut Catalog) {
        for patch in &self.extensions {
            patch(catalog);
        }
    }

    /// The carrier under the built-in packs: the catalog with no server pack.
    pub fn builtin(&self, carrier: &Catalog) -> Catalog {
        let mut catalog = carrier.clone();
        for patch in &self.builtin {
            patch(&mut catalog);
        }
        catalog
    }

    /// Server `layers`, bottom first, over the built-in catalog; bootstrap and reload workers
    /// build the same catalog.
    pub fn layer(&self, carrier: &Catalog, layers: &[Vec<(String, Vec<u8>)>]) -> LayeredCatalogs {
        let builtin = self.builtin(carrier);
        let mut catalog = builtin.clone();
        let mut restored = vec![false; self.restores.len()];
        for files in layers {
            for (done, rule) in restored.iter_mut().zip(&self.restores) {
                if !*done && (rule.authored)(&catalog, files) {
                    (rule.restore)(&mut catalog, carrier);
                    *done = true;
                }
            }
            catalog.apply_pack(
                files
                    .iter()
                    .map(|(path, bytes)| (path.as_str(), bytes.as_slice())),
            );
        }
        let mut screen_controls = Vec::new();
        for finish in &self.finishers {
            screen_controls.extend(finish(&mut catalog, &builtin));
        }
        for note in catalog
            .diagnostics()
            .iter()
            .skip(carrier.diagnostics().len())
        {
            tracing::debug!(note, "server ui pack");
        }
        // Built-in screens keep the built-in templates they inherit, so pack overrides of shared
        // ones cannot reach them.
        let builtin_screens = (!screen_controls.is_empty()).then(|| {
            let mut screens = builtin;
            for control in screen_controls {
                screens.insert(control);
            }
            screens
        });
        LayeredCatalogs {
            pack: catalog,
            builtin_screens,
        }
    }
}
