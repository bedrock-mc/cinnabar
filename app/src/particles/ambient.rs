//! Native fixed-tick leaf animation triggers.
//! Other block animateTick callbacks and unclassified native materials remain unsupported.

use std::time::{Duration, Instant};

use assets::{BLOCK_VISUAL_VARIANT_SEASONAL_LEAF, BlockFlags};
use client_world::WorldStream;
use render::{ParticleSystem, ParticleView};

use super::world_adapter::StreamParticleWorld;
use crate::movement::{MAX_LOCAL_PHYSICS_TICKS_PER_FRAME, PhysicsCollisionRegistries};

mod color;
mod diagnostics;
mod random;
mod sampler;
#[cfg(test)]
mod tests;

use random::AmbientRandom;
use sampler::Sampler;

pub(super) const LEAF_EFFECT: &str = "minecraft:biome_tinted_leaves_particle";
const LEAF_CHANCE_DENOMINATOR: u32 = 100;

#[derive(Default)]
pub(super) struct AmbientParticles {
    sampler: Sampler,
    random: AmbientRandom,
    accumulated_nanos: u128,
    diagnostics: diagnostics::Diagnostics,
}

impl AmbientParticles {
    pub(super) fn reset(&mut self) {
        self.sampler = Sampler::default();
        self.accumulated_nanos = 0;
        self.diagnostics = diagnostics::Diagnostics::default();
    }

    /// Vanilla LevelRenderer::tick calls animateTick once per
    /// world tick, not once per rendered frame. This clock must not depend on
    /// movement packet admission. Reuse the world tick duration and app catch-up bound.
    pub(super) fn drive(
        &mut self,
        elapsed: Duration,
        view: &ParticleView,
        stream: &WorldStream,
        world: &StreamParticleWorld<'_>,
        collisions: &PhysicsCollisionRegistries,
        system: &mut ParticleSystem,
    ) {
        let due = self.due_ticks(elapsed);
        let effect_present = system.has_effect(LEAF_EFFECT);
        let view_valid = valid_view(view);
        self.diagnostics.enabled = bevy::log::tracing::enabled!(
            target: "bedrock_client::ambient_leaves",
            bevy::log::Level::DEBUG
        );
        if due == 0 || !effect_present || !view_valid {
            self.report(elapsed, due, effect_present, view_valid, system);
            return;
        }
        for _ in 0..due {
            let started = Instant::now();
            let plan = self.sampler.plan(view.position, view.forward);
            for index in 0..plan.sample_count {
                let Some(block) = plan.sample(index, &mut self.random) else {
                    continue;
                };
                if self.diagnostics.enabled {
                    self.diagnostics.samples += 1;
                }
                self.try_leaf(block, stream, world, collisions, system);
            }
            self.sampler.finish(view.position, started.elapsed());
        }
        self.report(elapsed, due, effect_present, view_valid, system);
    }

    fn report(
        &mut self,
        elapsed: Duration,
        due: usize,
        effect_present: bool,
        view_valid: bool,
        system: &ParticleSystem,
    ) {
        self.diagnostics.report(
            elapsed,
            due,
            [effect_present, view_valid],
            self.sampler.sample_count,
            system,
        );
    }

    fn due_ticks(&mut self, elapsed: Duration) -> usize {
        self.accumulated_nanos = self.accumulated_nanos.saturating_add(elapsed.as_nanos());
        let tick_nanos = world::TICK_DURATION.as_nanos();
        let elapsed_ticks = self.accumulated_nanos / tick_nanos;
        self.accumulated_nanos %= tick_nanos;
        elapsed_ticks.min(MAX_LOCAL_PHYSICS_TICKS_PER_FRAME as u128) as usize
    }

    fn try_leaf(
        &mut self,
        block: [i32; 3],
        stream: &WorldStream,
        world: &StreamParticleWorld<'_>,
        collisions: &PhysicsCollisionRegistries,
        system: &mut ParticleSystem,
    ) {
        let Some(runtime_id) = world.block_runtime_id(block) else {
            return;
        };
        let mode = stream.network_id_mode();
        let leaf = stream.runtime_assets().resolve(mode, runtime_id);
        // This source-backed carrier route contains the seven standard biome
        // tinted families; specialized/fixed-colour leaves need their own params.
        if !leaf.is_known() || leaf.variant() & BLOCK_VISUAL_VARIANT_SEASONAL_LEAF == 0 {
            return;
        }
        if self.diagnostics.enabled {
            self.diagnostics.eligible += 1;
        }
        // Native rolls before looking beneath the block, including blocked leaves.
        if self.random.bounded(LEAF_CHANCE_DENOMINATOR) != 0 {
            return;
        }
        if self.diagnostics.enabled {
            self.diagnostics.roll_hits += 1;
        }
        let Some(y) = block[1].checked_sub(1) else {
            return;
        };
        let Some(below_id) = world.block_runtime_id([block[0], y, block[2]]) else {
            return;
        };
        let below = stream.runtime_assets().resolve(mode, below_id);
        let identifier = collisions.block_identifier(mode, below_id);
        if !below.is_known() || !material_allows_leaf(below.flags(), identifier) {
            return;
        }
        if self.diagnostics.enabled {
            self.diagnostics.below_admitted += 1;
        }
        let flags = stream
            .runtime_assets()
            .material(leaf.face(assets::BlockFace::Down).material_id())
            .flags;
        let color = color::leaf_tint(stream, world, flags, block);
        let before = self.diagnostics.enabled.then(|| system.live_particles());
        let accepted = system.spawn_biome_tinted(LEAF_EFFECT, block, color);
        if let Some(before) = before {
            self.diagnostics.accepted_requests += u64::from(accepted.is_some());
            self.diagnostics.emitted_lower_bound +=
                system.live_particles().saturating_sub(before) as u64;
        }
    }
}

fn valid_view(view: &ParticleView) -> bool {
    view.position
        .iter()
        .chain(&view.forward)
        .all(|component| component.is_finite())
}

/// Material::_setupMaterials: air(0) and plant(8)
/// are neither solid nor liquid. TallGrass, Flower,
/// Mushroom use plant; do not infer this from collision/Cross shape.
fn material_allows_leaf(flags: BlockFlags, identifier: Option<&str>) -> bool {
    flags.contains(BlockFlags::AIR)
        || matches!(
            identifier,
            Some(
                "minecraft:short_grass"
                    | "minecraft:fern"
                    | "minecraft:poppy"
                    | "minecraft:dandelion"
                    | "minecraft:brown_mushroom"
                    | "minecraft:red_mushroom"
            )
        )
}
