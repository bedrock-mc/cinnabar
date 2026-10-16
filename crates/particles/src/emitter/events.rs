//! Authored event interpretation, with admission before traversal and output ownership.

use std::sync::Arc;

use super::{Emitter, MAX_SPAWN_DEPTH, Outputs, ParticleSound, SpawnRequest};
use crate::def::{EventNode, SpawnKind};

impl Outputs {
    /// Supplies a small allowance to exercise admission without large authored fixtures.
    #[cfg(test)]
    pub(crate) fn with_limits(limits: crate::event_budget::EventLimits) -> Self {
        Self {
            budget: crate::event_budget::EventBudget::new(limits),
            ..Self::default()
        }
    }

    /// Retains the newest admitted sounds without temporarily exceeding the queue limit.
    fn sound(&mut self, name: &str, position: [f32; 3]) {
        let limit = self.budget.limits.sounds;
        if limit == 0 {
            self.budget.overflow.sounds = self.budget.overflow.sounds.saturating_add(1);
            return;
        }
        if self.sounds.len() == limit {
            self.sounds.pop_front();
            self.budget.overflow.sounds = self.budget.overflow.sounds.saturating_add(1);
        }
        self.sounds.push_back(ParticleSound {
            name: name.into(),
            position,
        });
    }
}

impl Emitter {
    /// Runs a named event, returning false when the shared allowance stops traversal.
    pub fn run_event(
        &mut self,
        name: &str,
        position: [f32; 3],
        velocity: [f32; 3],
        output: &mut Outputs,
    ) -> bool {
        if !output.budget.charge(1) {
            return false;
        }
        let def = Arc::clone(&self.def);
        for (key, node) in &def.events {
            if !output.budget.charge(1) {
                return false;
            }
            if key.as_ref() == name {
                return self.run_node(node, position, velocity, output);
            }
        }
        true
    }

    /// Interprets one admitted node, skipping unvisited siblings when work runs out.
    fn run_node(
        &mut self,
        node: &EventNode,
        position: [f32; 3],
        velocity: [f32; 3],
        output: &mut Outputs,
    ) -> bool {
        if !output.budget.charge(1) {
            return false;
        }
        match node {
            EventNode::Spawn { effect, kind } => {
                if self.depth >= MAX_SPAWN_DEPTH {
                    return true;
                }
                if output.spawns.len() >= output.budget.limits.spawns {
                    output.budget.overflow.spawns = output.budget.overflow.spawns.saturating_add(1);
                    return true;
                }
                let bound = match kind {
                    SpawnKind::EmitterBound => self.bound.map(|(actor, offset)| {
                        (
                            actor,
                            std::array::from_fn(|i| offset[i] + position[i] - self.pos[i]),
                        )
                    }),
                    _ => None,
                };
                output.spawns.push(SpawnRequest {
                    effect: effect.to_string(),
                    position,
                    bound,
                    basis: Some(self.basis),
                    queries: self.queries.clone(),
                    actor_attachment: if matches!(kind, SpawnKind::EmitterBound) {
                        self.actor_attachment.clone().map(|mut attachment| {
                            attachment.world_offset = std::array::from_fn(|axis| {
                                attachment.world_offset[axis] + position[axis] - self.pos[axis]
                            });
                            attachment
                        })
                    } else {
                        None
                    },
                    inherit_velocity: matches!(kind, SpawnKind::ParticleWithVelocity)
                        .then_some(velocity),
                    depth: self.depth + 1,
                    ..SpawnRequest::default()
                });
            }
            EventNode::Sound(name) => output.sound(name, position),
            EventNode::Expression(program) => {
                if !output.budget.charge(1) {
                    return false;
                }
                self.eval(program);
            }
            EventNode::Sequence(nodes) => {
                for (index, node) in nodes.iter().enumerate() {
                    if !self.run_node(node, position, velocity, output) {
                        output.budget.skip_work(nodes.len() - index - 1);
                        return false;
                    }
                }
            }
            EventNode::Randomize(options) => {
                // Both scans are reserved before reading the fanout or drawing from the RNG.
                if !output.budget.charge(options.len().saturating_mul(2)) {
                    return false;
                }
                let total: f32 = options.iter().map(|(weight, _)| weight).sum();
                if total <= 0.0 {
                    return true;
                }
                let mut pick = self.rng.unit() * total;
                for (weight, node) in options {
                    if pick < *weight {
                        return self.run_node(node, position, velocity, output);
                    }
                    pick -= weight;
                }
            }
        }
        true
    }
}
