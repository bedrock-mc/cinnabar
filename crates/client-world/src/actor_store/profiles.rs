use super::*;
use sha2::{Digest, Sha256};

/// Non-owning feed identity; the base raster already carries its ingest content hash.
pub(super) fn skin_fingerprint(skin: &PlayerSkin) -> [u8; 32] {
    let mut digest = Sha256::new();
    let PlayerSkin::Standard(skin) = skin else {
        let PlayerSkin::Unavailable(reason) = skin else {
            unreachable!()
        };
        digest.update([0, *reason as u8]);
        return digest.finalize().into();
    };
    digest.update([1]);
    digest.update(skin.width.to_le_bytes());
    digest.update(skin.height.to_le_bytes());
    digest.update((skin.rgba8.len() as u64).to_le_bytes());
    digest.update(skin.rgba8.content_hash().to_le_bytes());
    digest.update([u8::from(skin.cape.is_some())]);
    if let Some(cape) = &skin.cape {
        digest.update(cape.width.to_le_bytes());
        digest.update(cape.height.to_le_bytes());
        fingerprint_bytes(&mut digest, &cape.rgba8);
    }
    digest.update([u8::from(skin.geometry.is_some())]);
    if let Some(geometry) = &skin.geometry {
        fingerprint_bytes(&mut digest, geometry.resource_patch.as_bytes());
        fingerprint_bytes(&mut digest, geometry.geometry_data.as_bytes());
        digest.update((geometry.animations.len() as u64).to_le_bytes());
        for animation in geometry.animations.iter() {
            digest.update([animation.kind.slot() as u8, u8::from(animation.blinking)]);
            digest.update(animation.width.to_le_bytes());
            digest.update(animation.height.to_le_bytes());
            digest.update(animation.frames.to_le_bytes());
            fingerprint_bytes(&mut digest, &animation.rgba8);
        }
    }
    digest.finalize().into()
}

fn fingerprint_bytes(digest: &mut Sha256, bytes: &[u8]) {
    digest.update((bytes.len() as u64).to_le_bytes());
    digest.update(bytes);
}

impl ActorStore {
    /// Chooses an unused local identity without replacing a retained server profile.
    pub(super) fn available_profile_uuid(&self, preferred: [u8; 16]) -> [u8; 16] {
        let mut candidate = preferred;
        while self.players.contains_key(&candidate)
            || self.unlisted_players.contains_key(&candidate)
        {
            candidate = u128::from_le_bytes(candidate).wrapping_add(1).to_le_bytes();
        }
        candidate
    }

    /// Inserts or replaces a profile while preserving roster and retained-skin limits.
    pub(super) fn upsert_profile(&mut self, uuid: [u8; 16], mut profile: PlayerProfile) -> bool {
        if self.players.len() >= self.max_players && !self.players.contains_key(&uuid) {
            return false;
        }
        // An unlisted profile still holds its retained skin until the actor despawns.
        let previous = self
            .players
            .get(&uuid)
            .or_else(|| self.unlisted_players.get(&uuid));
        let previous_skin_bytes = previous.map_or(0, |profile| retained_skin_bytes(&profile.skin));
        let retained_without_previous = self
            .retained_player_skin_bytes
            .saturating_sub(previous_skin_bytes);
        let requested_skin_bytes = retained_skin_bytes(&profile.skin);
        let (skin, retained_player_skin_bytes) = retained_without_previous
            .checked_add(requested_skin_bytes)
            .filter(|total| *total <= self.max_player_skin_bytes)
            .map_or_else(
                || {
                    previous.map_or_else(
                        || {
                            (
                                PlayerSkin::Unavailable(
                                    PlayerSkinUnavailable::RetainedBudgetExceeded,
                                ),
                                retained_without_previous,
                            )
                        },
                        |profile| {
                            (
                                profile.skin.clone(),
                                retained_without_previous.saturating_add(previous_skin_bytes),
                            )
                        },
                    )
                },
                |total| (profile.skin.clone(), total),
            );
        self.retained_player_skin_bytes = retained_player_skin_bytes;

        profile.skin = skin;
        if let Some(unique_id) = self.spawned_player_unique_id(&uuid) {
            profile.unique_id = unique_id;
        }
        self.unlisted_players.remove(&uuid);
        self.players.insert(uuid, profile);
        true
    }

    /// Removes a profile and releases its retained skin charge.
    pub(super) fn remove_profile(&mut self, uuid: &[u8; 16]) {
        if let Some(profile) = self
            .players
            .remove(uuid)
            .or_else(|| self.unlisted_players.remove(uuid))
        {
            self.retained_player_skin_bytes = self
                .retained_player_skin_bytes
                .saturating_sub(retained_skin_bytes(&profile.skin));
        }
    }
}
