//! Reads HUD images, sidecars and animation frames on the reload worker before first use.

use std::collections::{BTreeMap, BTreeSet};

use json_ui::TextureMeta;

use super::frame_sidecars::{FrameSidecars, Frames};
use super::{PackTextures, ServerAtlas, Source};

/// First-use reads of named pack textures, as the atlas makes them.
#[derive(Default)]
pub struct PrereadTextures {
    images: BTreeMap<String, Option<Source>>,
    sidecars: BTreeMap<String, Option<TextureMeta>>,
    /// Animation frames the pack itself has; a miss still falls back to the local vanilla pack.
    frames: BTreeMap<String, Frames>,
}

impl std::fmt::Debug for PrereadTextures {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PrereadTextures")
            .field("images", &self.images.len())
            .field("sidecars", &self.sidecars.len())
            .field("frames", &self.frames.len())
            .finish()
    }
}

impl PrereadTextures {
    /// Reads `keys` from `view` as an atlas over it reads them on first use.
    pub(super) fn read(view: &resource_pack::LayeredPackView, keys: &BTreeSet<String>) -> Self {
        let pack = PackTextures::index(view.clone());
        let frames = FrameSidecars::new(&[], Some(view.clone()));
        Self {
            images: keys
                .iter()
                .map(|key| (key.clone(), pack.image(key)))
                .collect(),
            sidecars: keys
                .iter()
                .map(|key| (key.clone(), pack.sidecar(key)))
                .collect(),
            frames: keys
                .iter()
                .filter_map(|key| Some((key.clone(), frames.get(key)?)))
                .collect(),
        }
    }
}

impl ServerAtlas {
    /// Starts from the reads a worker already made over this atlas's pack stack.
    pub(in crate::ui_runtime::presentation::forms) fn with_preread(
        mut self,
        preread: Option<&PrereadTextures>,
    ) -> Self {
        let Some(preread) = preread else {
            return self;
        };
        if let Some(pack) = &self.pack {
            pack.loaded.borrow_mut().extend(
                preread
                    .images
                    .iter()
                    .map(|(key, source)| (key.clone(), source.clone())),
            );
            pack.loaded_sidecars.borrow_mut().extend(
                preread
                    .sidecars
                    .iter()
                    .map(|(key, sidecar)| (key.clone(), *sidecar)),
            );
        }
        self.frames.seed(&preread.frames);
        self
    }
}
