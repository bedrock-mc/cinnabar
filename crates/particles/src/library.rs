//! Parsed effect definitions keyed by identifier.

use std::{collections::HashMap, sync::Arc};

use assets::RuntimeParticleAssets;

use super::def::{EffectDef, parse_effect};

#[derive(Default)]
pub struct EffectLibrary {
    effects: HashMap<Box<str>, Arc<EffectDef>>,
}

impl EffectLibrary {
    #[must_use]
    pub fn from_assets(assets: &RuntimeParticleAssets) -> Self {
        let mut library = Self::default();
        for file in assets.effects() {
            library.insert(&file.bytes);
        }
        library
    }

    /// Adds or replaces an effect (joined server packs override vanilla ones).
    pub fn insert(&mut self, bytes: &[u8]) -> bool {
        match parse_effect(bytes) {
            Some(effect) => {
                self.effects
                    .insert(effect.identifier.clone(), Arc::new(effect));
                true
            }
            None => false,
        }
    }

    #[must_use]
    pub fn get(&self, identifier: &str) -> Option<&Arc<EffectDef>> {
        self.effects.get(identifier)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.effects.len()
    }
}
