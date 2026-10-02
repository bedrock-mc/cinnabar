use super::{ActorApplyResult, ActorStore, PlayerSkin, retained_skin_bytes};

impl ActorStore {
    /// Replaces only a known player's appearance, preserving the roster and retained-byte budget.
    pub(super) fn apply_skin_update(
        &mut self,
        uuid: [u8; 16],
        skin: PlayerSkin,
    ) -> ActorApplyResult {
        let Some(profile) = self
            .players
            .get_mut(&uuid)
            .or_else(|| self.unlisted_players.get_mut(&uuid))
        else {
            return ActorApplyResult::MissingActor;
        };
        if matches!(skin, PlayerSkin::Unavailable(_)) {
            return ActorApplyResult::CapacityRejected;
        }
        let retained = self.retained_player_skin_bytes - retained_skin_bytes(&profile.skin);
        let Some(total) = retained
            .checked_add(retained_skin_bytes(&skin))
            .filter(|total| *total <= self.max_player_skin_bytes)
        else {
            return ActorApplyResult::CapacityRejected;
        };
        profile.skin = skin;
        self.retained_player_skin_bytes = total;
        ActorApplyResult::Updated
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{
        ActorEvent, CLASSIC_SKIN_SIDE, PlayerListEntry, PlayerListUpdateEvent, StandardSkin,
    };
    use std::sync::Arc;

    /// A small valid appearance with a recognisable pixel value.
    fn skin(value: u8) -> PlayerSkin {
        PlayerSkin::Standard(StandardSkin {
            width: CLASSIC_SKIN_SIDE as u32,
            height: CLASSIC_SKIN_SIDE as u32,
            rgba8: vec![value; CLASSIC_SKIN_SIDE * CLASSIC_SKIN_SIDE * 4].into(),
            cape: None,
            geometry: None,
        })
    }

    #[test]
    fn player_skin_updates_preserve_roster_identity_and_account_for_replacement() {
        let mut store =
            ActorStore::with_limits(1, 0, 4, 4, CLASSIC_SKIN_SIDE * CLASSIC_SKIN_SIDE * 4);
        let uuid = [7; 16];
        let add = ActorEvent::PlayerList(PlayerListUpdateEvent {
            entries: Arc::from([PlayerListEntry::Add {
                uuid,
                unique_id: 42,
                username: "fixture".into(),
                verified: true,
                skin: skin(1),
            }]),
        });
        store.apply(1, 1, add);
        assert_eq!(
            store.apply(
                1,
                2,
                ActorEvent::Skin {
                    uuid,
                    skin: skin(2)
                }
            ),
            ActorApplyResult::Updated
        );
        let profile = &store.players[&uuid];
        assert_eq!(profile.skin, skin(2));
        assert_eq!(profile.unique_id, 42);
        assert_eq!(&*profile.username, "fixture");
        assert!(profile.verified);
        assert_eq!(
            store.retained_player_skin_bytes,
            CLASSIC_SKIN_SIDE * CLASSIC_SKIN_SIDE * 4
        );
        assert_eq!(
            store.apply(
                1,
                3,
                ActorEvent::Skin {
                    uuid: [8; 16],
                    skin: skin(3)
                }
            ),
            ActorApplyResult::MissingActor
        );
        assert_eq!(store.players.len(), 1);
        assert_eq!(
            store.apply(
                1,
                4,
                ActorEvent::Skin {
                    uuid,
                    skin: PlayerSkin::Unavailable(
                        protocol::PlayerSkinUnavailable::InvalidDimensions
                    )
                }
            ),
            ActorApplyResult::CapacityRejected
        );
        assert_eq!(store.players[&uuid].skin, skin(2));
        let PlayerSkin::Standard(mut larger) = skin(3) else {
            unreachable!();
        };
        larger.cape = Some(protocol::CapeImage {
            width: 64,
            height: 32,
            rgba8: vec![255; 64 * 32 * 4].into(),
        });
        assert_eq!(
            store.apply(
                1,
                5,
                ActorEvent::Skin {
                    uuid,
                    skin: PlayerSkin::Standard(larger)
                }
            ),
            ActorApplyResult::CapacityRejected
        );
        assert_eq!(store.players[&uuid].skin, skin(2));
        assert_eq!(
            store.retained_player_skin_bytes,
            CLASSIC_SKIN_SIDE * CLASSIC_SKIN_SIDE * 4
        );
    }
}
