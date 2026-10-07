//! A roster removal does not replace a spawned player's skin or model.

use super::*;

impl ActorStore {
    /// Removes the roster entry, keeping its appearance while the matching actor exists.
    pub(super) fn unlist_player(&mut self, uuid: &[u8; 16]) {
        let Some(profile) = self.players.remove(uuid) else {
            return;
        };
        if self.actors.values().any(|actor| {
            actor.unique_id == profile.unique_id
                && matches!(&actor.kind, ActorKind::Player { uuid: actor_uuid, .. }
                    if actor_uuid == uuid || (self.remote_state_excluded_runtime_id == Some(actor.runtime_id)
                        && self.synthetic_local_uuid == Some(*actor_uuid)))
        }) {
            self.unlisted_players.insert(*uuid, profile);
        } else {
            self.retained_player_skin_bytes = self.retained_player_skin_bytes
                .saturating_sub(retained_skin_bytes(&profile.skin));
        }
    }

    /// Releases unlisted appearances when their actor lifetime ends, including replacements.
    pub(super) fn prune_unlisted_players(&mut self) {
        self.unlisted_players.retain(|uuid, profile| {
            let live = self.actors.values().any(|actor| {
                actor.unique_id == profile.unique_id
                    && matches!(&actor.kind, ActorKind::Player { uuid: actor_uuid, .. }
                    if actor_uuid == uuid || (self.remote_state_excluded_runtime_id == Some(actor.runtime_id)
                        && self.synthetic_local_uuid == Some(*actor_uuid)))
            });
            if !live {
                self.retained_player_skin_bytes = self.retained_player_skin_bytes
                    .saturating_sub(retained_skin_bytes(&profile.skin));
            }
            live
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{ActorRemoveEvent, CLASSIC_SKIN_SIDE, PlayerListUpdateEvent, StandardSkin};
    use std::sync::Arc;

    /// Lists a player with an identifiable skin and a retained custom-model source.
    fn add(byte: u8) -> ActorEvent {
        ActorEvent::PlayerList(PlayerListUpdateEvent {
            entries: Arc::from([PlayerListEntry::Add {
                uuid: [1; 16],
                unique_id: 1,
                username: "player".into(),
                verified: true,
                skin: PlayerSkin::Standard(StandardSkin {
                    width: CLASSIC_SKIN_SIDE as u32,
                    height: CLASSIC_SKIN_SIDE as u32,
                    rgba8: vec![byte; CLASSIC_SKIN_SIDE * CLASSIC_SKIN_SIDE * 4].into(),
                    cape: None,
                    geometry: Some(Arc::new(protocol::SkinGeometrySource {
                        resource_patch: "patch".into(),
                        geometry_data: "model".into(),
                        animations: Arc::from([]),
                    })),
                }),
            }]),
        })
    }

    /// Spawns the listed player under its current actor lifetime.
    fn spawn() -> ActorEvent {
        ActorEvent::Spawn(protocol::ActorSpawnEvent {
            dimension: 0,
            unique_id: 1,
            runtime_id: 2,
            kind: ActorKind::Player {
                uuid: [1; 16],
                username: "player".into(),
            },
            position: [0.0, 64.0, 0.0],
            velocity: [0.0; 3],
            pitch: 0.0,
            yaw: 0.0,
            head_yaw: 0.0,
            body_yaw: 0.0,
            held_item: Default::default(),
            metadata: Arc::from([]),
            attributes: Arc::from([]),
            properties: Arc::from([]),
            links: Arc::from([]),
        })
    }

    /// Removes the player's roster entry without removing the actor.
    fn unlist() -> ActorEvent {
        ActorEvent::PlayerList(PlayerListUpdateEvent {
            entries: Arc::from([PlayerListEntry::Remove { uuid: [1; 16] }]),
        })
    }

    #[test]
    fn roster_removal_keeps_spawned_skin_and_model_until_actor_despawn() {
        let mut store = ActorStore::new(1, 0);
        store.apply(1, 1, add(9));
        store.apply(1, 2, spawn());
        let skin = store.player_profile(2).unwrap().skin.clone();
        let retained = store.retained_player_skin_bytes;
        store.apply(1, 3, unlist());
        assert_eq!(store.player_count(), 0);
        assert_eq!(store.player_profile(2).unwrap().skin, skin);
        assert_eq!(store.retained_player_skin_bytes, retained);
        store.apply(1, 4, unlist());
        assert_eq!(store.retained_player_skin_bytes, retained);
        store.apply(
            1,
            5,
            ActorEvent::Remove(ActorRemoveEvent {
                dimension: 0,
                unique_id: 1,
            }),
        );
        assert!(store.unlisted_players.is_empty());
        assert_eq!(store.retained_player_skin_bytes, 0);
    }

    #[test]
    fn relisting_and_skin_updates_replace_one_retained_appearance_without_double_accounting() {
        let mut store = ActorStore::with_limits(1, 0, 4, 1, 100_000);
        store.apply(1, 1, add(1));
        store.apply(1, 2, spawn());
        let retained = store.retained_player_skin_bytes;
        store.apply(1, 3, unlist());
        let ActorEvent::PlayerList(update) = add(2) else {
            unreachable!()
        };
        let PlayerListEntry::Add { skin, .. } = &update.entries[0] else {
            unreachable!()
        };
        assert_eq!(
            store.apply(
                1,
                4,
                ActorEvent::Skin {
                    uuid: [1; 16],
                    skin: skin.clone()
                }
            ),
            ActorApplyResult::Updated
        );
        assert_eq!(store.player_profile(2).unwrap().skin, *skin);
        assert_eq!(store.retained_player_skin_bytes, retained);
        store.apply(1, 5, add(3));
        assert_eq!(store.player_count(), 1);
        assert!(store.unlisted_players.is_empty());
        assert_eq!(store.retained_player_skin_bytes, retained);
        store.apply(1, 6, unlist());
        store.reset_dimension(1, 7, 1);
        assert!(store.unlisted_players.is_empty());
        assert_eq!(store.retained_player_skin_bytes, 0);
    }

    #[test]
    fn unspawned_roster_removal_releases_appearance_immediately() {
        let mut store = ActorStore::new(1, 0);
        store.apply(1, 1, add(9));
        store.apply(1, 2, unlist());
        assert!(store.unlisted_players.is_empty());
        assert_eq!(store.retained_player_skin_bytes, 0);
    }
    #[test]
    fn replacing_an_actor_releases_its_unlisted_appearance() {
        let mut store = ActorStore::new(1, 0);
        store.apply(1, 1, add(9));
        store.apply(1, 2, spawn());
        store.apply(1, 3, unlist());
        let ActorEvent::Spawn(mut replacement) = spawn() else {
            unreachable!()
        };
        replacement.unique_id = 3;
        replacement.kind = ActorKind::Player {
            uuid: [3; 16],
            username: "replacement".into(),
        };
        store.apply(1, 4, ActorEvent::Spawn(replacement));
        assert!(store.unlisted_players.is_empty());
        assert_eq!(store.retained_player_skin_bytes, 0);
    }
}
