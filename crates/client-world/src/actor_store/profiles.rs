use super::*;
use sha2::{Digest, Sha256};
use std::sync::Arc;

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

/// The last fingerprinted skin's identity and fingerprint, so an unchanged feed is not rehashed.
/// The base raster is keyed by its ingest content hash. The cape and model inputs are keyed by
/// allocation address: the weak references reserve those addresses, so an equal address proves
/// the same immutable bytes, without keeping the skin alive.
#[derive(Debug, Default)]
pub(super) struct SkinFingerprintMemo {
    last: Option<FingerprintedSkin>,
}

#[derive(Debug)]
struct FingerprintedSkin {
    base: (u32, u32, usize, u64),
    cape: Option<(u32, u32, std::sync::Weak<[u8]>)>,
    geometry: Option<std::sync::Weak<render_api::SkinGeometrySource>>,
    fingerprint: [u8; 32],
}

impl SkinFingerprintMemo {
    /// [`skin_fingerprint`] of `skin`, reusing the previous result when `skin` has the same
    /// base raster hash and the same cape and model allocations.
    pub(super) fn fingerprint(&mut self, skin: &PlayerSkin) -> [u8; 32] {
        let PlayerSkin::Standard(standard) = skin else {
            return skin_fingerprint(skin);
        };
        let base = (
            standard.width,
            standard.height,
            standard.rgba8.len(),
            standard.rgba8.content_hash(),
        );
        if let Some(last) = &self.last
            && last.base == base
            && match (&standard.cape, &last.cape) {
                (None, None) => true,
                (Some(cape), Some((width, height, bytes))) => {
                    cape.width == *width
                        && cape.height == *height
                        && std::ptr::addr_eq(Arc::as_ptr(&cape.rgba8), bytes.as_ptr())
                }
                _ => false,
            }
            && match (&standard.geometry, &last.geometry) {
                (None, None) => true,
                (Some(geometry), Some(last)) => std::ptr::eq(Arc::as_ptr(geometry), last.as_ptr()),
                _ => false,
            }
        {
            return last.fingerprint;
        }
        let fingerprint = skin_fingerprint(skin);
        self.last = Some(FingerprintedSkin {
            base,
            cape: standard
                .cape
                .as_ref()
                .map(|cape| (cape.width, cape.height, Arc::downgrade(&cape.rgba8))),
            geometry: standard.geometry.as_ref().map(Arc::downgrade),
            fingerprint,
        });
        fingerprint
    }

    /// Forgets the remembered skin, releasing its reserved allocations.
    pub(super) fn clear(&mut self) {
        self.last = None;
    }
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
    /// `spawned` is a roster update's [`Self::spawned_player_unique_ids`], taken while the actor
    /// set is unchanged; `None` searches the actors for this UUID instead.
    pub(super) fn upsert_profile(
        &mut self,
        uuid: [u8; 16],
        mut profile: PlayerProfile,
        spawned: Option<&FastHashMap<[u8; 16], i64>>,
    ) -> bool {
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
        let spawned_unique_id = match spawned {
            Some(ids) => ids.get(&uuid).copied(),
            None => self.spawned_player_unique_id(&uuid),
        };
        if let Some(unique_id) = spawned_unique_id {
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
