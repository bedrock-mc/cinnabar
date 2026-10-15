//! Parsed effect definitions keyed by identifier.

use std::{collections::HashMap, sync::Arc};

use assets::{RuntimeParticleAssets, facts::ParticleEffect};

use super::def::{EffectDef, parse_effect};

pub struct EffectLibrary {
    effects: HashMap<Box<str>, Arc<EffectDef>>,
    native: Box<[Option<Arc<EffectDef>>]>,
}

impl Default for EffectLibrary {
    /// Allocates the numeric native-effect cache before any source is admitted.
    fn default() -> Self {
        Self {
            effects: HashMap::new(),
            native: vec![None; ParticleEffect::ALL.len()].into_boxed_slice(),
        }
    }
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
                let native = ParticleEffect::from_name(&effect.identifier);
                self.effects
                    .insert(effect.identifier.clone(), Arc::new(effect));
                if let Some(native) = native {
                    self.native[native as usize] = self
                        .effects
                        .get(native.name())
                        .or_else(|| {
                            self.effects
                                .get(format!("minecraft:{}", native.name()).as_str())
                        })
                        .cloned();
                }
                true
            }
            None => false,
        }
    }

    /// Resolves a compiled effect in one array lookup, including pack overrides.
    pub fn get_native(&self, effect: ParticleEffect) -> Option<&Arc<EffectDef>> {
        self.native.get(effect as usize)?.as_ref()
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Supplies a real pinned effect identity with only the schema needed for library admission.
    fn document(identifier: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "particle_effect": {
                "description": {"identifier": identifier, "basic_render_parameters": {
                    "material": "particles_alpha", "texture": "textures/particle/particles"
                }},
                "components": {
                    "minecraft:particle_lifetime_expression": {"max_lifetime": 1},
                    "minecraft:particle_appearance_billboard": {}
                }
            }
        }))
        .unwrap()
    }

    #[test]
    fn native_handles_refresh_on_pack_override_and_preserve_bare_name_precedence() {
        let effect = ParticleEffect::BasicFlameParticle;
        let namespaced = format!("minecraft:{}", effect.name());
        let mut library = EffectLibrary::default();
        assert!(library.insert(&document(&namespaced)));
        let first = library.get_native(effect).unwrap().clone();
        assert!(library.insert(&document(&namespaced)));
        assert!(!Arc::ptr_eq(&first, library.get_native(effect).unwrap()));
        assert!(library.insert(&document(effect.name())));
        let bare = library.get_native(effect).unwrap().clone();
        assert!(library.insert(&document(&namespaced)));
        assert!(Arc::ptr_eq(&bare, library.get_native(effect).unwrap()));
    }
}
