//! Terrain texture keys and flipbooks resolved across the stack.

use std::{collections::HashMap, sync::Arc};

use assets::texture::flipbook::FlipbookSource;
use resource_pack::LayeredPackView;

use pack_runtime::pack_textures::DecodedTexture;
mod carrier;
mod identity;
pub(super) use identity::{TextureIdentity, TexturePath, TextureStorageKey};
mod decoding;
mod diagnostics;
mod frames;
pub(super) mod terrain;
pub(super) use carrier::{carrier_image, carrier_image_path};
use diagnostics::{FailureReason, TextureDiagnostics};
#[cfg(test)]
pub(super) use frames::shrink_to_max;
pub(super) use frames::{FrameSequence, flipbook_frames, resample_square};
use pack_runtime::pack_textures::normalize_texture_path;
pub(super) use terrain::{admit_static_rectangle, source_mip_chain};

/// Pixels supplied by either a pack image or the immutable carrier.
pub(super) struct TextureImage {
    pub(super) texture: DecodedTexture,
    pub(super) image_path: String,
    pub(super) baked_tint: Option<[u8; 3]>,
    pub(super) pack: bool,
}

/// Texture keys resolved across the stack; a higher pack replaces a key.
pub(super) struct TextureCatalog<'a> {
    view: &'a LayeredPackView,
    data: CatalogData,
    pub(super) texture_policy: assets::texture::policy::TexturePolicy,
    #[cfg(test)]
    pub(super) decode_attempts: std::cell::Cell<usize>,
}

/// Parsed terrain inputs can be rebound to each subscriber's independent read recorder.
#[derive(Clone)]
struct CatalogData {
    terrain: Arc<HashMap<String, pack_runtime::pack_textures::terrain::Entry>>,
    base_terrain: Arc<HashMap<String, pack_runtime::pack_textures::terrain::Entry>>,
    flipbooks: Arc<HashMap<String, Vec<FlipbookSource>>>,
    destinations: Arc<HashMap<String, Vec<u32>>>,
    diagnostics: TextureDiagnostics,
}

/// Shares parsed tables within one blocks/icons compile, retaining their read dependencies.
#[derive(Default)]
pub(in crate::runtime::network) struct SharedTextures {
    parsed: std::cell::RefCell<Option<PreparedTextures>>,
    #[cfg(test)]
    pub(in crate::runtime::network) catalog_builds: std::cell::Cell<usize>,
}

struct PreparedTextures {
    data: CatalogData,
    files: std::collections::BTreeSet<resource_pack::PackDependency>,
}

impl<'a> TextureCatalog<'a> {
    pub(super) fn new(
        view: &'a LayeredPackView,
        base: Option<&assets::material_keys::MaterialKeys>,
    ) -> Self {
        let mut terrain = pack_runtime::pack_textures::base_terrain_catalog()
            .into_iter()
            .filter_map(|(key, path)| {
                let entry = pack_runtime::pack_textures::terrain::parse_entry(
                    &serde_json::json!({"textures":path}),
                )?;
                Some((key, entry))
            })
            .collect::<HashMap<_, _>>();
        if let Some(base) = base {
            let fixed_tints = base.fixed_tints().collect::<HashMap<_, _>>();
            for (key, path) in base.aliases() {
                if let Some(entry) = pack_runtime::pack_textures::terrain::parse_entry(
                    &serde_json::json!({"textures":path}),
                ) {
                    terrain.insert(
                        key.to_owned(),
                        entry.with_base_tint(fixed_tints.get(key).copied()),
                    );
                }
            }
            for (key, value) in base.terrain_entries() {
                if let Some(entry) = pack_runtime::pack_textures::terrain::parse_entry(value) {
                    terrain.insert(key.to_owned(), entry);
                }
            }
        }
        let server_terrain = pack_runtime::pack_textures::terrain::entries(view);
        let diagnostics = TextureDiagnostics::default();
        diagnostics.catalogs(view);
        let destinations = pack_runtime::pack_flipbooks::destinations(view, base);
        let mut flipbooks = HashMap::<String, Vec<FlipbookSource>>::new();
        for mut source in pack_runtime::pack_flipbooks::flipbooks(
            view,
            base.map_or(&[], assets::material_keys::MaterialKeys::flipbooks),
        ) {
            let Some(count) = destinations
                .get(source.atlas_tile.as_ref())
                .and_then(|counts| counts.get(source.atlas_index as usize))
            else {
                diagnostics.failure(
                    FailureReason::AnimationDestinationMissing,
                    &source.atlas_tile,
                    Some(&source.texture_path),
                );
                continue;
            };
            source.atlas_tile_variant = normalize_variant(source.atlas_tile_variant, *count);
            flipbooks
                .entry(source.atlas_tile.to_string())
                .or_default()
                .push(source);
        }
        Self {
            view,
            data: CatalogData {
                terrain: Arc::new(server_terrain),
                base_terrain: Arc::new(terrain),
                flipbooks: Arc::new(flipbooks),
                destinations: Arc::new(destinations),
                diagnostics,
            },
            texture_policy: assets::texture::policy::device_policy(),
            #[cfg(test)]
            decode_attempts: std::cell::Cell::new(0),
        }
    }

    /// Reuses parsed catalog inputs while replaying their dependencies into this subscriber.
    pub(super) fn reusing(
        view: &'a LayeredPackView,
        base: Option<&assets::material_keys::MaterialKeys>,
        shared: Option<&SharedTextures>,
    ) -> Self {
        let Some(shared) = shared else {
            return Self::new(view, base);
        };
        if let Some(PreparedTextures { data, files }) = shared.parsed.borrow().as_ref() {
            if let Some(dependencies) = view.dependencies() {
                dependencies.extend(files.clone());
            }
            return Self {
                view,
                data: data.clone(),
                texture_policy: assets::texture::policy::device_policy(),
                #[cfg(test)]
                decode_attempts: std::cell::Cell::new(0),
            };
        }
        #[cfg(test)]
        shared.catalog_builds.set(shared.catalog_builds.get() + 1);
        let catalog = Self::new(view, base);
        if let Some(dependencies) = view.dependencies() {
            *shared.parsed.borrow_mut() = Some(PreparedTextures {
                data: catalog.data.clone(),
                files: dependencies.snapshot(),
            });
        }
        catalog
    }

    /// Returns the last animation created for a destination, including all-variant descriptions.
    pub(super) fn flipbook(&self, key: &str, selector: (u32, u32)) -> Option<&FlipbookSource> {
        let selector = self.normalize_selector(key, selector);
        self.data.flipbooks.get(key)?.iter().rev().find(|source| {
            source.atlas_index == selector.0
                && (source.all_variants || source.atlas_tile_variant == selector.1)
        })
    }

    /// Clamps material indices to the winning layout, then normalizes their UV variants.
    pub(super) fn normalize_selector(&self, key: &str, selector: (u32, u32)) -> (u32, u32) {
        let Some(counts) = self
            .data
            .destinations
            .get(key)
            .filter(|counts| !counts.is_empty())
        else {
            return selector;
        };
        let index = selector.0.min(counts.len() as u32 - 1);
        (index, normalize_variant(selector.1, counts[index as usize]))
    }

    /// Returns the winning terrain entry's normalized UV-grid exponent.
    pub(super) fn grid(&self, key: &str) -> u8 {
        self.entry(key).map_or(0, |entry| entry.grid)
    }

    /// Whether the winning state choices or UV grid differ from the immutable base catalog.
    pub(super) fn redefined(&self, key: &str, variant: u32) -> bool {
        self.data.terrain.get(key).is_some_and(|entry| {
            self.data.base_terrain.get(key).is_none_or(|base| {
                entry.grid != base.grid || entry.variant(variant) != base.variant(variant)
            })
        })
    }

    /// Resolves a complete entry from the optional stack before falling back to the base.
    fn entry(&self, key: &str) -> Option<&pack_runtime::pack_textures::terrain::Entry> {
        self.data
            .terrain
            .get(key)
            .or_else(|| self.data.base_terrain.get(key))
    }

    /// Choices for one state variant, independent of the other variants.
    pub(super) fn choices(
        &self,
        key: &str,
        variant: u32,
    ) -> Option<&[pack_runtime::pack_textures::terrain::Choice]> {
        Some(self.entry(key)?.variant(variant))
    }

    /// Resolves the state index used for selector caching and animation eligibility.
    pub(super) fn resolved_variant(&self, key: &str, variant: u32) -> Option<u32> {
        Some(self.entry(key)?.resolved_variant(variant))
    }

    /// Decodes a state choice through the shared raster, tint, and image-source resolution.
    #[cfg(test)]
    pub(super) fn decode_choice(
        &self,
        key: &str,
        variant: u32,
        choice: u32,
    ) -> Option<DecodedTexture> {
        self.decode_route(
            &super::TextureRoute::Catalog {
                key: key.to_owned(),
                variant,
                choice,
            },
            |_, _| None,
        )
        .map(|image| image.texture)
    }

    /// Returns the resolved state and choice indices together with the winning raster policy.
    fn resolve_choice(
        &self,
        route: &super::TextureRoute,
    ) -> Option<(u32, usize, &pack_runtime::pack_textures::terrain::Choice)> {
        use pack_runtime::pack_textures::texture_image_paths;
        let entry = self.entry(route.key())?;
        let (variant, choice) = match route {
            super::TextureRoute::Catalog {
                variant, choice, ..
            } => (entry.resolved_variant(*variant), *choice as usize),
            super::TextureRoute::Path {
                variant,
                path,
                choice,
                ..
            } => {
                let variant = entry.resolved_variant(*variant);
                let selected = entry
                    .variant(variant)
                    .get(*choice as usize)
                    .or_else(|| entry.variant(variant).first())?;
                let paths = texture_image_paths(path);
                if !texture_image_paths(&selected.path)
                    .iter()
                    .any(|path| paths.contains(path))
                {
                    return None;
                }
                (variant, *choice as usize)
            }
        };
        Some((
            variant,
            choice,
            entry
                .variant(variant)
                .get(choice)
                .or_else(|| entry.variant(variant).first())?,
        ))
    }

    /// Retains every pixel or animation property that can change an admitted raster slot.
    pub(super) fn identity(&self, route: &super::TextureRoute) -> Option<TextureIdentity> {
        let Some((resolved_variant, index, choice)) = self.resolve_choice(route) else {
            self.data
                .diagnostics
                .failure(FailureReason::TerrainKeyMissing, route.key(), None);
            return None;
        };
        let requested_variant = match route {
            super::TextureRoute::Catalog { variant, .. }
            | super::TextureRoute::Path { variant, .. } => *variant,
        };
        let variant = if self.data.terrain.contains_key(route.key())
            || self
                .data
                .destinations
                .get(route.key())
                .is_some_and(|counts| {
                    self.entry(route.key())
                        .is_some_and(|entry| counts.len() == entry.variant_count())
                }) {
            resolved_variant
        } else {
            requested_variant
        };
        let selector = self.normalize_selector(route.key(), (variant, index as u32));
        let flipbook = self.flipbook(route.key(), selector).cloned();
        Some(TextureIdentity {
            path: TexturePath::Catalog(normalize_texture_path(&choice.path).to_ascii_lowercase()),
            tint: choice.tint,
            grid: self.grid(route.key()),
            selector: if flipbook.is_some() { selector } else { (0, 0) },
            flipbook,
        })
    }

    /// Identifies immutable frames without their destination's playback timing.
    pub(super) fn storage_key(
        &self,
        route: &super::TextureRoute,
        source: Option<&FlipbookSource>,
    ) -> Option<TextureStorageKey> {
        let (_, _, choice) = self.resolve_choice(route)?;
        let path = source.map_or(choice.path.as_str(), |source| source.texture_path.as_ref());
        Some(TextureStorageKey {
            path: TexturePath::Catalog(normalize_texture_path(path).to_ascii_lowercase()),
            grid: self.grid(route.key()),
            tint: choice.tint,
            frames: source.map(|source| (source.frames.clone(), source.replicate)),
        })
    }

    /// Records a route the overlay could not admit because its texture array is full.
    pub(super) fn report_full(&self, key: &str) {
        self.data
            .diagnostics
            .failure(FailureReason::TextureArrayFull, key, None);
    }

    /// Names a route whose full-resolution storage would exceed the page budget.
    pub(super) fn report_budget(&self, key: &str) {
        self.data
            .diagnostics
            .failure(FailureReason::TextureBudgetExceeded, key, None);
    }

    /// Emits the counted terrain failures for this stack.
    pub(super) fn emit_diagnostics(&self) {
        self.data.diagnostics.emit();
    }
}

/// Uses variant zero when the requested UV set is outside the destination's bounds.
fn normalize_variant(variant: u32, count: u32) -> u32 {
    if variant < count { variant } else { 0 }
}
