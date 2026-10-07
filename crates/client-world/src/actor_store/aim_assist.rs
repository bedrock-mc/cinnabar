use super::{ActorApplyResult, ActorSnapshot, ActorStore};
use protocol::ActorKind;

impl ActorStore {
    /// The first advertised definition wins; unrecognized or empty bases construct generic mobs.
    pub(super) fn apply_aim_actor_classes(
        &mut self,
        registry: protocol::ActorIdentifierRegistry,
    ) -> ActorApplyResult {
        let previous_skips = self.actor_identifier_skips;
        self.actor_identifier_skips += registry.skipped;
        for entry in registry.entries.iter() {
            if native_class(&entry.identifier).is_some()
                || self.aim_actor_classes.contains_key(&entry.identifier)
            {
                continue;
            }
            if self.aim_actor_classes.len() == protocol::MAX_ACTOR_IDENTIFIERS {
                self.actor_identifier_skips += 1;
                continue;
            }
            self.aim_actor_classes.insert(
                entry.identifier.clone(),
                native_class(&entry.base_identifier).unwrap_or(true),
            );
        }
        if self.actor_identifier_skips != previous_skips {
            eprintln!(
                "ignored unsupported actor identifiers (total {})",
                self.actor_identifier_skips
            );
        }
        ActorApplyResult::Updated
    }

    /// Custom actor classes and remote-player spectator state are retained at packet admission.
    pub(crate) fn camera_aim_assist_eligible(&self, actor: &ActorSnapshot) -> Option<bool> {
        match &actor.kind {
            ActorKind::Player { .. } => Some(
                !matches!(
                    actor.player_game_mode,
                    Some(protocol::GameModeUpdate::Explicit(
                        protocol::PlayerGameMode::Spectator
                    ))
                ) && (actor.player_game_mode != Some(protocol::GameModeUpdate::WorldDefault)
                    || self.world_default_game_mode != Some(protocol::PlayerGameMode::Spectator)),
            ),
            ActorKind::Entity { identifier } => {
                native_class(identifier).or_else(|| self.aim_actor_classes.get(identifier).copied())
            }
        }
    }

    /// Remote mode changes affect target admission without becoming local HUD changes.
    pub(crate) fn apply_player_game_mode(
        &mut self,
        unique_id: i64,
        mode: protocol::GameModeUpdate,
    ) {
        if matches!(
            mode,
            protocol::GameModeUpdate::Unknown(_)
                | protocol::GameModeUpdate::Explicit(protocol::PlayerGameMode::Unknown)
        ) {
            self.player_game_mode_skips += 1;
            eprintln!(
                "ignored unsupported player game mode (total {})",
                self.player_game_mode_skips
            );
            return;
        }
        if let Some(actor) = self
            .unique_to_runtime
            .get(&unique_id)
            .and_then(|id| self.actors.get_mut(id))
            && matches!(actor.kind, ActorKind::Player { .. })
        {
            actor.player_game_mode = Some(mode);
        }
    }

    /// The level default is resolved at lookup, so existing default-bound players follow changes.
    pub(crate) fn apply_world_game_mode(&mut self, mode: protocol::GameModeUpdate) {
        match mode {
            protocol::GameModeUpdate::Explicit(mode)
                if mode != protocol::PlayerGameMode::Unknown =>
            {
                self.world_default_game_mode = Some(mode);
            }
            protocol::GameModeUpdate::LegacyViewer => {
                self.world_default_game_mode = None;
            }
            _ => {}
        }
    }
}

/// Unknown custom identifiers need their runtime class before they can be admitted.
fn native_class(identifier: &str) -> Option<bool> {
    let identifier = identifier.split('<').next()?;
    match identifier.strip_prefix("minecraft:").unwrap_or(identifier) {
        "agent"
        | "allay"
        | "armadillo"
        | "armor_stand"
        | "axolotl"
        | "bat"
        | "bee"
        | "blaze"
        | "boat"
        | "bogged"
        | "breeze"
        | "camel"
        | "camel_husk"
        | "cat"
        | "cave_spider"
        | "chest_boat"
        | "chest_minecart"
        | "chicken"
        | "cod"
        | "command_block_minecart"
        | "copper_golem"
        | "cow"
        | "creaking"
        | "creeper"
        | "dolphin"
        | "donkey"
        | "drowned"
        | "elder_guardian"
        | "elder_guardian_ghost"
        | "ender_dragon"
        | "enderman"
        | "endermite"
        | "evocation_illager"
        | "fox"
        | "frog"
        | "ghast"
        | "glow_squid"
        | "goat"
        | "guardian"
        | "happy_ghast"
        | "hoglin"
        | "hopper_minecart"
        | "horse"
        | "husk"
        | "iron_golem"
        | "llama"
        | "magma_cube"
        | "minecart"
        | "mooshroom"
        | "mule"
        | "nautilus"
        | "npc"
        | "ocelot"
        | "panda"
        | "parched"
        | "parrot"
        | "phantom"
        | "pig"
        | "piglin"
        | "piglin_brute"
        | "pillager"
        | "player"
        | "polar_bear"
        | "pufferfish"
        | "rabbit"
        | "ravager"
        | "salmon"
        | "sheep"
        | "shulker"
        | "silverfish"
        | "skeleton"
        | "skeleton_horse"
        | "slime"
        | "sniffer"
        | "snow_golem"
        | "spider"
        | "squid"
        | "stray"
        | "strider"
        | "sulfur_cube"
        | "tadpole"
        | "tnt_minecart"
        | "trader_llama"
        | "tripod_camera"
        | "tropicalfish"
        | "turtle"
        | "vex"
        | "villager"
        | "villager_v2"
        | "vindicator"
        | "wandering_trader"
        | "warden"
        | "witch"
        | "wither"
        | "wither_skeleton"
        | "wolf"
        | "zoglin"
        | "zombie"
        | "zombie_horse"
        | "zombie_nautilus"
        | "zombie_pigman"
        | "zombie_villager"
        | "zombie_villager_v2" => Some(true),
        "area_effect_cloud"
        | "arrow"
        | "balloon"
        | "chalkboard"
        | "dragon_fireball"
        | "egg"
        | "ender_crystal"
        | "ender_pearl"
        | "evocation_fang"
        | "eye_of_ender_signal"
        | "falling_block"
        | "fireball"
        | "fireworks_rocket"
        | "fishing_hook"
        | "ice_bomb"
        | "item"
        | "leash_knot"
        | "lightning_bolt"
        | "lingering_potion"
        | "llama_spit"
        | "moving_block"
        | "ominous_item_spawner"
        | "painting"
        | "shield"
        | "shulker_bullet"
        | "small_fireball"
        | "snowball"
        | "splash_potion"
        | "thrown_trident"
        | "tnt"
        | "wind_charge_projectile"
        | "wither_skull"
        | "wither_skull_dangerous"
        | "xp_bottle"
        | "xp_orb" => Some(false),
        "breeze_wind_charge_projectile" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::native_class;

    #[test]
    fn native_class_admits_mobs_boats_and_minecarts_only() {
        for identifier in [
            "minecraft:player",
            "minecraft:pig",
            "minecraft:armor_stand",
            "minecraft:chest_boat",
            "minecraft:tnt_minecart",
        ] {
            assert_eq!(native_class(identifier), Some(true));
        }
        for identifier in [
            "minecraft:item",
            "minecraft:arrow",
            "minecraft:painting",
            "minecraft:tnt",
        ] {
            assert_eq!(native_class(identifier), Some(false));
        }
        assert_eq!(native_class("pig"), Some(true));
        for identifier in ["custom:pig", "minecraft:unknown"] {
            assert_eq!(native_class(identifier), None);
        }
    }

    #[test]
    fn custom_bases_use_first_advertisement_and_unknown_bases_are_mobs() {
        use protocol::{ActorIdentifier, ActorIdentifierRegistry};
        use std::sync::Arc;
        let mut store = super::ActorStore::new(1, 0);
        for entries in [
            vec![
                ("custom:arrow", "minecraft:arrow"),
                ("custom:mob", ""),
                ("custom:unknown", "custom:base"),
            ],
            vec![
                ("custom:arrow", "minecraft:pig"),
                ("minecraft:pig", "minecraft:arrow"),
            ],
        ] {
            store.apply_aim_actor_classes(ActorIdentifierRegistry {
                entries: entries
                    .into_iter()
                    .map(|(id, base)| ActorIdentifier {
                        identifier: Arc::from(id),
                        base_identifier: Arc::from(base),
                    })
                    .collect(),
                skipped: 0,
            });
        }
        assert_eq!(store.aim_actor_classes.get("custom:arrow"), Some(&false));
        assert_eq!(store.aim_actor_classes.get("custom:mob"), Some(&true));
        assert_eq!(store.aim_actor_classes.get("custom:unknown"), Some(&true));
        assert!(!store.aim_actor_classes.contains_key("minecraft:pig"));
        store.reset_dimension(1, 1, 1);
        assert_eq!(store.aim_actor_classes.get("custom:arrow"), Some(&false));
        store.begin_session(2, 0);
        assert!(store.aim_actor_classes.is_empty());
    }
    #[test]
    fn spectator_players_follow_spawn_updates_and_level_default_without_losing_mode_on_unknown() {
        use protocol::{ActorEvent, ActorKind, GameModeUpdate, PlayerGameMode};
        let mut store = super::ActorStore::new(1, 0);
        let ActorEvent::Spawn(mut spawn) = crate::actor_store::tests::spawn(9, -9) else {
            unreachable!();
        };
        spawn.kind = ActorKind::Player {
            uuid: [0; 16],
            username: "test".into(),
        };
        store.apply(
            1,
            1,
            ActorEvent::PlayerSpawn {
                spawn,
                game_mode: GameModeUpdate::Explicit(PlayerGameMode::Spectator),
            },
        );
        assert_eq!(
            store.camera_aim_assist_eligible(store.get(9).unwrap()),
            Some(false)
        );
        store.apply_player_game_mode(-9, GameModeUpdate::Explicit(PlayerGameMode::Survival));
        assert_eq!(
            store.camera_aim_assist_eligible(store.get(9).unwrap()),
            Some(true)
        );
        store.apply_world_game_mode(GameModeUpdate::Explicit(PlayerGameMode::Spectator));
        assert_eq!(
            store.camera_aim_assist_eligible(store.get(9).unwrap()),
            Some(true)
        );
        store.apply_player_game_mode(-9, GameModeUpdate::LegacyViewer);
        assert_eq!(
            store.camera_aim_assist_eligible(store.get(9).unwrap()),
            Some(true)
        );
        store.apply_player_game_mode(-9, GameModeUpdate::WorldDefault);
        assert_eq!(
            store.camera_aim_assist_eligible(store.get(9).unwrap()),
            Some(false)
        );
        store.apply_player_game_mode(-9, GameModeUpdate::Unknown(99));
        assert_eq!(store.player_game_mode_skips, 1);
        assert_eq!(
            store.camera_aim_assist_eligible(store.get(9).unwrap()),
            Some(false)
        );
        store.apply_world_game_mode(GameModeUpdate::LegacyViewer);
        assert_eq!(
            store.camera_aim_assist_eligible(store.get(9).unwrap()),
            Some(true)
        );
        store.apply_world_game_mode(GameModeUpdate::Explicit(PlayerGameMode::Creative));
        assert_eq!(
            store.camera_aim_assist_eligible(store.get(9).unwrap()),
            Some(true)
        );
        store.reset_dimension(1, 1, 2);
        assert_eq!(
            store.world_default_game_mode,
            Some(PlayerGameMode::Creative)
        );
        store.begin_session(2, 0);
        assert_eq!(store.world_default_game_mode, None);
    }
}
