use super::{ActorApplyResult, ActorSnapshot, ActorStore};
use protocol::ActorKind;

impl ActorStore {
    /// Admits the first advertised base and refreshes actors that arrived before its definition.
    pub(super) fn apply_actor_interactions(
        &mut self,
        registry: protocol::ActorIdentifierRegistry,
    ) -> ActorApplyResult {
        let previous_skips = self.actor_identifier_skips;
        self.apply_entity_bases(&registry);
        self.actor_identifier_skips += registry.skipped;
        for actor in self.actors.values_mut() {
            actor.interaction_traits = Self::resolve_actor_interactions(
                &actor.kind,
                self.entity_assets.as_deref(),
                &self.custom_actor_facts,
            );
        }
        if self.actor_identifier_skips != previous_skips {
            tracing::warn!(
                "ignored unsupported actor identifiers (total {})",
                self.actor_identifier_skips
            );
        }
        ActorApplyResult::Updated
    }

    /// Resolves a constructor disposition once; diagnostics use the same compatibility source.
    pub(super) fn resolve_actor_interactions(
        kind: &ActorKind,
        assets: Option<&assets::RuntimeEntityAssets>,
        custom: &std::collections::HashMap<std::sync::Arc<str>, assets::facts::entity::EntityFact>,
    ) -> assets::interactions::EntityInteractionTraits {
        use assets::interactions::{self, EntityInteractionTraits};
        match kind {
            ActorKind::Player { .. } => EntityInteractionTraits::GENERIC_MOB,
            ActorKind::Entity { identifier } => {
                let traits = assets.map_or_else(
                    || interactions::builtin(identifier),
                    |assets| assets.interaction_traits_for(identifier),
                );
                match traits {
                    EntityInteractionTraits::Unresolved => custom
                        .get(identifier)
                        .map_or(EntityInteractionTraits::Unresolved, |fact| {
                            fact.interactions
                        }),
                    resolved => resolved,
                }
            }
        }
    }

    /// Custom actor classes and remote-player spectator state are retained at packet admission.
    pub(crate) fn camera_aim_assist_eligible(&self, actor: &ActorSnapshot) -> Option<bool> {
        match &actor.kind {
            ActorKind::Player { .. } => Some(!actor.is_spectator(self.world_default_game_mode)),
            ActorKind::Entity { .. } => actor.interaction_traits.aim_assist(),
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
            tracing::warn!(
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
    /// The world's default game mode, when known.
    pub(crate) const fn world_default_game_mode(&self) -> Option<protocol::PlayerGameMode> {
        self.world_default_game_mode
    }

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

#[cfg(test)]
mod tests {

    #[test]
    fn compatibility_traits_preserve_existing_aim_routes() {
        for identifier in [
            "minecraft:player",
            "minecraft:pig",
            "minecraft:armor_stand",
            "minecraft:chest_boat",
            "minecraft:tnt_minecart",
        ] {
            assert_eq!(
                assets::interactions::builtin(identifier).aim_assist(),
                Some(true)
            );
        }
        for identifier in [
            "minecraft:item",
            "minecraft:arrow",
            "minecraft:painting",
            "minecraft:tnt",
        ] {
            assert_eq!(
                assets::interactions::builtin(identifier).aim_assist(),
                Some(false)
            );
        }
        assert_eq!(
            assets::interactions::builtin("pig").aim_assist(),
            Some(true)
        );
        for identifier in ["custom:pig", "minecraft:unknown"] {
            assert_eq!(assets::interactions::builtin(identifier).aim_assist(), None);
        }
    }

    #[test]
    fn custom_bases_preserve_first_advertisement_and_unadmitted_bases_stay_unresolved() {
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
            store.apply_actor_interactions(ActorIdentifierRegistry {
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
        assert_eq!(
            store
                .custom_actor_facts
                .get("custom:arrow")
                .and_then(|fact| fact.interactions.aim_assist()),
            Some(false)
        );
        assert_eq!(
            store
                .custom_actor_facts
                .get("custom:mob")
                .and_then(|fact| fact.interactions.aim_assist()),
            Some(true)
        );
        assert_eq!(
            store
                .custom_actor_facts
                .get("custom:unknown")
                .and_then(|fact| fact.interactions.aim_assist()),
            None
        );
        assert!(!store.custom_actor_facts.contains_key("minecraft:pig"));
        store.reset_dimension(1, 1, 1);
        assert_eq!(
            store
                .custom_actor_facts
                .get("custom:arrow")
                .and_then(|fact| fact.interactions.aim_assist()),
            Some(false)
        );
        store.begin_session(2, 0);
        assert!(store.custom_actor_facts.is_empty());
    }
    #[test]
    fn interaction_admission_refreshes_earlier_actors_and_preserves_first_base() {
        use protocol::{ActorEvent, ActorIdentifier, ActorIdentifierRegistry, ActorKind};
        use std::sync::Arc;
        let mut store = super::ActorStore::new(1, 0);
        let ActorEvent::Spawn(mut spawn) = crate::actor_store::tests::spawn(8, -8) else {
            unreachable!()
        };
        spawn.kind = ActorKind::Entity {
            identifier: "fixture:projectile".into(),
        };
        store.apply(1, 1, ActorEvent::Spawn(spawn.clone()));
        assert_eq!(store.get(8).unwrap().interaction_traits.pickable(), None);
        for base in ["minecraft:arrow", "minecraft:pig"] {
            store.apply_actor_interactions(ActorIdentifierRegistry {
                entries: Arc::from([ActorIdentifier {
                    identifier: "fixture:projectile".into(),
                    base_identifier: base.into(),
                }]),
                skipped: 0,
            });
            let actor = store.get(8).unwrap();
            assert_eq!(actor.interaction_traits.pickable(), Some(false));
            assert_eq!(actor.interaction_traits.obstructs_placement(), Some(false));
            assert_eq!(store.camera_aim_assist_eligible(actor), Some(false));
        }
        spawn.kind = ActorKind::Entity {
            identifier: "minecraft:pig".into(),
        };
        store.apply(1, 2, ActorEvent::Spawn(spawn));
        assert_eq!(
            store.get(8).unwrap().interaction_traits.pickable(),
            Some(true)
        );
        let mut detached = store.get(8).unwrap().clone();
        detached.kind = ActorKind::Entity {
            identifier: "fixture:display".into(),
        };
        assert_eq!(store.camera_aim_assist_eligible(&detached), Some(true));
    }

    #[test]
    fn interaction_bases_resolve_from_a_checked_carrier() {
        use protocol::{ActorEvent, ActorIdentifier, ActorIdentifierRegistry, ActorKind};
        use std::sync::Arc;
        let compiled = assets::carriers::startup::test_carriers::synthetic_interaction_assets(
            "fixture:projectile",
        );
        let encoded = assets::encode_entity_blob(&compiled).unwrap();
        let runtime = Arc::new(assets::RuntimeEntityAssets::decode(&encoded).unwrap());
        let mut store = super::ActorStore::new_with_entity_assets(1, 0, runtime);
        let ActorEvent::Spawn(mut spawn) = crate::actor_store::tests::spawn(8, -8) else {
            unreachable!()
        };
        spawn.kind = ActorKind::Entity {
            identifier: "fixture:projectile".into(),
        };
        store.apply(1, 1, ActorEvent::Spawn(spawn));
        assert_eq!(store.get(8).unwrap().interaction_traits.pickable(), None);
        store.apply_actor_interactions(ActorIdentifierRegistry {
            entries: Arc::from([ActorIdentifier {
                identifier: "fixture:projectile".into(),
                base_identifier: "minecraft:arrow".into(),
            }]),
            skipped: 0,
        });
        let actor = store.get(8).unwrap();
        assert_eq!(actor.interaction_traits.pickable(), Some(false));
        assert_eq!(actor.interaction_traits.obstructs_placement(), Some(false));
        assert_eq!(store.camera_aim_assist_eligible(actor), Some(false));
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
