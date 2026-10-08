//! Vanilla entity-shadow casters: who casts one, its radius and where it hangs.
//! Rules are tabulated in `docs/reference/entity-shadows.md`.
use protocol::{ActorKind, ActorMetadataValue};

use super::{
    ActorSnapshot, ActorStore, BOUNDING_BOX_HEIGHT_METADATA_KEY, BOUNDING_BOX_WIDTH_METADATA_KEY,
    DEFAULT_ACTOR_COLLISION_WIDTH, FLAG_BABY, VARIANT_METADATA_KEY,
};

/// Actor types flagged as projectiles, which never cast a shadow.
const PROJECTILES: [&str; 19] = [
    "arrow",
    "breeze_wind_charge_projectile",
    "dragon_fireball",
    "egg",
    "ender_pearl",
    "evocation_fang",
    "fireball",
    "ice_bomb",
    "lingering_potion",
    "llama_spit",
    "shulker_bullet",
    "small_fireball",
    "snowball",
    "splash_potion",
    "thrown_trident",
    "wind_charge_projectile",
    "wither_skull",
    "wither_skull_dangerous",
    "xp_bottle",
];

/// One caster: interpolated feet, lowered by any authored offset, and its radius.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActorShadowCaster {
    pub runtime_id: u64,
    pub feet: [f32; 3],
    pub radius: f32,
}

impl ActorSnapshot {
    /// The radius vanilla gives this actor's shadow; zero casts none.
    #[must_use]
    pub fn shadow_radius(&self, riding: bool) -> f32 {
        let path = match &self.kind {
            ActorKind::Player { .. } => return self.collision_width(),
            ActorKind::Entity { identifier } => {
                identifier.strip_prefix("minecraft:").unwrap_or(identifier)
            }
        };
        let width = self.collision_width();
        let baby = self.flag(FLAG_BABY);
        match path {
            "ghast" | "happy_ghast" | "creaking" => width * 0.8,
            "spider" | "cave_spider" => width * 0.7,
            "armadillo" => width * 0.575,
            "horse" | "donkey" | "mule" | "skeleton_horse" | "zombie_horse" => width * 0.6,
            "ender_dragon" => width * 0.3,
            "tadpole" => width * 0.5,
            "iron_golem" | "shulker" => width * 0.5 * if baby { 0.5 } else { 1.0 },
            "turtle" if baby => width * 0.33,
            "slime" | "magma_cube" | "sulfur_cube" => self.variant() as f32 * 0.25,
            "tripod_camera" | "ender_crystal" => 0.5,
            "boat" | "chest_boat" => 1.0,
            "parrot" if riding => 0.0,
            "armor_stand"
            | "area_effect_cloud"
            | "fishing_hook"
            | "minecart"
            | "chest_minecart"
            | "hopper_minecart"
            | "tnt_minecart"
            | "command_block_minecart"
            | "xp_orb"
            | "leash_knot"
            | "eye_of_ender_signal"
            | "lightning_bolt"
            | "tnt"
            | "falling_block"
            | "fireworks_rocket"
            | "painting" => 0.0,
            _ => width,
        }
    }

    /// Whether this actor's own state hides its shadow.
    #[must_use]
    pub fn shadow_hidden(&self) -> bool {
        let projectile = matches!(&self.kind, ActorKind::Entity { identifier }
            if identifier.strip_prefix("minecraft:").is_some_and(|path| PROJECTILES.contains(&path)));
        let dead = self.status.dead
            || self
                .attributes
                .get("minecraft:health")
                .is_some_and(|health| health.current <= 0.0);
        projectile
            || dead
            || self.is_on_fire()
            || self.is_invisible()
            || self.status.breathing_submerged == Some(true)
    }

    /// Height the shadow hangs below the feet; ghasts carry authored relative offsets.
    fn shadow_drop(&self) -> f32 {
        let relative = match &self.kind {
            ActorKind::Entity { identifier } if identifier.as_ref() == "minecraft:ghast" => -0.875,
            ActorKind::Entity { identifier } if identifier.as_ref() == "minecraft:happy_ghast" => {
                -0.5
            }
            _ => return 0.0,
        };
        let height = match self.metadata.get(&BOUNDING_BOX_HEIGHT_METADATA_KEY) {
            Some(ActorMetadataValue::Float(height)) if height.is_finite() => *height,
            _ => 0.0,
        };
        relative * height * self.render_scale()
    }

    fn collision_width(&self) -> f32 {
        match self.metadata.get(&BOUNDING_BOX_WIDTH_METADATA_KEY) {
            Some(ActorMetadataValue::Float(width)) if width.is_finite() && *width > 0.0 => *width,
            _ if matches!(self.kind, ActorKind::Player { .. }) => DEFAULT_ACTOR_COLLISION_WIDTH,
            _ => 0.0,
        }
    }

    fn variant(&self) -> i32 {
        match self.metadata.get(&VARIANT_METADATA_KEY) {
            Some(ActorMetadataValue::Int(variant)) => (*variant).max(0),
            _ => 0,
        }
    }
}

impl ActorStore {
    /// The actor's caster at `alpha` of the frame, if it casts one; an actor riding a visible
    /// vehicle leaves the shadow to it.
    pub(crate) fn shadow_caster(
        &self,
        actor: &ActorSnapshot,
        alpha: f32,
    ) -> Option<ActorShadowCaster> {
        let vehicle = self
            .ridden_unique_id(actor.unique_id)
            .and_then(|vehicle| self.snapshot_by_unique(vehicle));
        if actor.shadow_hidden() || vehicle.is_some_and(|vehicle| !vehicle.is_invisible()) {
            return None;
        }
        let radius = actor.shadow_radius(vehicle.is_some());
        if !(radius > 0.0 && radius.is_finite()) {
            return None;
        }
        let mut feet = actor.interpolated_position(alpha.clamp(0.0, 1.0))?;
        feet[1] += actor.shadow_drop();
        Some(ActorShadowCaster {
            runtime_id: actor.runtime_id,
            feet,
            radius,
        })
    }

    pub(crate) fn shadow_casters(
        &self,
        alpha: f32,
    ) -> impl Iterator<Item = ActorShadowCaster> + '_ {
        self.actors
            .values()
            .filter_map(move |actor| self.shadow_caster(actor, alpha))
    }

    /// Records `(runtime_id, submerged)` breathing-point samples that hide shadows.
    pub(crate) fn set_breathing_liquids(&mut self, samples: &[(u64, bool)]) {
        for &(runtime_id, submerged) in samples {
            if let Some(actor) = self.actors.get_mut(&runtime_id) {
                actor.status.breathing_submerged = Some(submerged);
            }
        }
    }
}

#[cfg(test)]
mod tests;
