//! Effective creation permission, retaining accepted mode and ability event order.
use crate::game_mode_capabilities::{ability_bit, ability_layer};
use protocol::{AbilityLayersEvidence, PlayerGameMode};

#[derive(Clone, Copy, Debug)]
enum Layer {
    Unset,
    Value(bool),
    Unavailable,
}

#[derive(Clone, Debug)]
pub(super) struct InstabuildLayers([Layer; ability_layer::COUNT]);

impl Default for InstabuildLayers {
    fn default() -> Self {
        let mut layers = [Layer::Unset; ability_layer::COUNT];
        layers[ability_layer::BASE] = Layer::Value(false);
        Self(layers)
    }
}

impl InstabuildLayers {
    /// Replaces every layer, including definitions omitted by the latest accepted packet.
    pub(super) fn replace(&mut self, update: &AbilityLayersEvidence) {
        let AbilityLayersEvidence::Received(layers) = update else {
            self.0.fill(Layer::Unavailable);
            return;
        };
        self.0.fill(Layer::Unset);
        for layer in layers.iter() {
            if let Some(target) = self.0.get_mut(usize::from(layer.layer_type)) {
                *target = if layer.abilities & ability_bit::INSTANT_BUILD == 0 {
                    Layer::Unset
                } else {
                    Layer::Value(layer.values & ability_bit::INSTANT_BUILD != 0)
                };
            }
        }
    }

    /// Rebuilds the mode-owned layer and applies the mode's base-layer assignment.
    pub(super) fn assign_mode(&mut self, mode: PlayerGameMode) {
        self.0[ability_layer::SPECTATOR] = if mode == PlayerGameMode::Spectator {
            Layer::Value(false)
        } else {
            Layer::Unset
        };
        match mode {
            PlayerGameMode::Creative => self.0[ability_layer::BASE] = Layer::Value(true),
            PlayerGameMode::Survival | PlayerGameMode::Adventure => {
                self.0[ability_layer::BASE] = Layer::Value(false);
            }
            PlayerGameMode::Spectator | PlayerGameMode::Unknown => {}
        }
    }

    /// Resolves the highest defined layer, retaining unavailable higher-layer evidence.
    pub(super) fn resolve(&self) -> Option<bool> {
        for layer in self.0.iter().rev() {
            match layer {
                Layer::Unset => {}
                Layer::Value(value) => return Some(*value),
                Layer::Unavailable => return None,
            }
        }
        Some(false)
    }
}
