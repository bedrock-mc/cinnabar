//! Authored controller-state particle aliases, resolved against each client entity.

use std::collections::BTreeMap;

use serde_json::Value;

mod lifecycle;
pub use lifecycle::{ActorEffectKey, ActorEffectTracker};

#[derive(Clone, Debug, Eq, PartialEq)]
struct EffectReference {
    alias: Box<str>,
    bound: bool,
}

/// Particle bindings accompany the optional pack effect catalog without changing its carrier.
#[derive(Default)]
pub struct ActorEffectBindings {
    entities: BTreeMap<Box<str>, BTreeMap<Box<str>, Box<str>>>,
    controllers: BTreeMap<Box<str>, BTreeMap<Box<str>, Vec<EffectReference>>>,
    /// Authored fragments whose locator or initialization expression is not supported.
    pub unsupported: u64,
}

impl ActorEffectBindings {
    /// Replaces the winning client entity's aliases, including an explicitly empty set.
    pub fn insert_entity(&mut self, root: &Value) {
        let description = &root["minecraft:client_entity"]["description"];
        let Some(identifier) = name(&description["identifier"]) else {
            return;
        };
        if self.entities.len() >= assets::MAX_ENTITY_ASSET_SYMBOLS
            && !self.entities.contains_key(identifier)
        {
            self.unsupported += 1;
            return;
        }
        let aliases = description["particle_effects"]
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(alias, value)| Some((name_str(alias)?.into(), name(value)?.into())))
            .take(assets::MAX_ENTITY_DEPENDENCIES)
            .collect();
        self.entities.insert(identifier.into(), aliases);
    }

    /// Replaces each controller definition's state effects in pack priority order.
    pub fn insert_controllers(&mut self, root: &Value) {
        for (identifier, controller) in root["animation_controllers"]
            .as_object()
            .into_iter()
            .flatten()
        {
            let Some(identifier) = name_str(identifier) else {
                continue;
            };
            if self.controllers.len() >= assets::MAX_ENTITY_CONTROLLERS
                && !self.controllers.contains_key(identifier)
            {
                self.unsupported += 1;
                continue;
            }
            let mut states = BTreeMap::new();
            for (state, definition) in controller["states"]
                .as_object()
                .into_iter()
                .flatten()
                .take(assets::MAX_ENTITY_CONTROLLER_STATES)
            {
                let Some(state) = name_str(state) else {
                    continue;
                };
                let mut effects = Vec::new();
                for effect in definition["particle_effects"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .take(assets::MAX_ENTITY_DEPENDENCIES)
                {
                    let Some(alias) = name(&effect["effect"]) else {
                        self.unsupported += 1;
                        continue;
                    };
                    if nonempty(&effect["locator"]) || nonempty(&effect["pre_effect_script"]) {
                        self.unsupported += 1;
                        continue;
                    }
                    effects.push(EffectReference {
                        alias: alias.into(),
                        bound: effect["bind_to_actor"].as_bool().unwrap_or(true),
                    });
                }
                states.insert(state.into(), effects);
            }
            self.controllers.insert(identifier.into(), states);
        }
    }

    /// Resolved effect identifiers in authored order; missing aliases produce no emitter.
    pub fn effects_for<'a>(
        &'a self,
        entity: &str,
        controller: &str,
        state: &str,
    ) -> impl Iterator<Item = (&'a str, bool)> + 'a {
        let aliases = self.entities.get(entity);
        self.controllers
            .get(controller)
            .and_then(|states| states.get(state))
            .into_iter()
            .flatten()
            .filter_map(move |effect| {
                Some((aliases?.get(effect.alias.as_ref())?.as_ref(), effect.bound))
            })
    }
}

fn name(value: &Value) -> Option<&str> {
    name_str(value.as_str()?)
}

fn name_str(value: &str) -> Option<&str> {
    (!value.is_empty() && value.len() <= assets::MAX_ENTITY_IDENTIFIER_BYTES).then_some(value)
}

fn nonempty(value: &Value) -> bool {
    !value.is_null() && value.as_str().is_none_or(|text| !text.is_empty())
}
