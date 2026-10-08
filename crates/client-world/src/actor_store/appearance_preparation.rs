//! Publishes an actor's pixels, cape and model together after immutable worker preparation.
use super::{ActorKind, ActorStore, PlayerProfile, PlayerSkin, retained_skin_bytes};
use std::{collections::HashMap, sync::Arc};

#[derive(Debug, Default)]
pub(super) struct ReadyAppearances {
    pub(super) profiles: HashMap<[u8; 16], PlayerProfile>,
    bytes: usize,
}

impl ActorStore {
    /// Ready profiles have their own byte ceiling, including replacements retained during a job.
    pub(crate) fn prepare_appearances(&mut self, publish: bool) {
        if !self.animation.has_skin_preparation() {
            return;
        }
        self.animation.begin_skin_preparation();
        let ready = &mut self.ready_appearances;
        ready.profiles.retain(|uuid, profile| {
            let present = self.players.get(uuid).or_else(|| self.unlisted_players.get(uuid))
                .is_some_and(|current| current.unique_id == profile.unique_id)
                && self.unique_to_runtime.get(&profile.unique_id)
                    .and_then(|runtime| self.actors.get(runtime))
                    .is_some_and(|actor| matches!(&actor.kind, ActorKind::Player { uuid: current, .. } if current == uuid));
            if !present { ready.bytes -= retained_skin_bytes(&profile.skin); }
            present
        });
        for profile in ready.profiles.values() {
            if let Some(source) = geometry_source(profile) {
                self.animation.request_skin_preparation(source);
            }
        }
        let local = self.remote_state_excluded_runtime_id;
        let actors = local
            .and_then(|runtime| self.actors.get(&runtime))
            .into_iter()
            .chain(
                self.actors
                    .values()
                    .filter(|actor| Some(actor.runtime_id) != local),
            );
        for actor in actors {
            let ActorKind::Player { uuid, .. } = &actor.kind else {
                continue;
            };
            let Some(profile) = self
                .players
                .get(uuid)
                .or_else(|| self.unlisted_players.get(uuid))
                .filter(|profile| profile.unique_id == actor.unique_id)
            else {
                continue;
            };
            let previous_source = ready.profiles.get(uuid).and_then(geometry_source);
            let prepared = geometry_source(profile).is_none_or(|source| {
                self.animation
                    .request_replacing_skin_preparation(source, previous_source)
            });
            if !publish || !prepared {
                continue;
            }
            let previous = ready.profiles.get(uuid);
            if previous.is_some_and(|previous| same_profile_allocation(previous, profile)) {
                continue;
            }
            let bytes = ready
                .bytes
                .saturating_sub(previous.map_or(0, |p| retained_skin_bytes(&p.skin)))
                .saturating_add(retained_skin_bytes(&profile.skin));
            if bytes > self.max_player_skin_bytes {
                continue;
            }
            ready.bytes = bytes;
            ready.profiles.insert(*uuid, profile.clone());
        }
        self.animation.submit_skin_preparation();
    }

    /// Tests explicitly await their own immutable job without assuming any wall-clock duration.
    #[cfg(test)]
    pub(crate) fn finish_appearance_fixture_batch(&mut self) {
        self.animation.finish_skin_fixture_batch();
    }

    pub(crate) fn appearance_preparation_pending(&self) -> bool {
        self.animation.skin_preparation_pending()
    }

    /// Newly pending appearances have no drawable body or hand until the matching model is ready.
    pub(super) fn appearance_ready(&self, actor: &super::ActorSnapshot) -> bool {
        let ActorKind::Player { uuid, .. } = &actor.kind else {
            return true;
        };
        !self.animation.has_skin_preparation()
            || self
                .ready_appearances
                .profiles
                .get(uuid)
                .is_some_and(|profile| profile.unique_id == actor.unique_id)
            || self
                .players
                .get(uuid)
                .or_else(|| self.unlisted_players.get(uuid))
                .is_none()
    }
}

/// Standard and animated skin bytes keep the source allocation alive until publication.
fn geometry_source(profile: &PlayerProfile) -> Option<&Arc<protocol::SkinGeometrySource>> {
    match &profile.skin {
        PlayerSkin::Standard(skin) => skin.geometry.as_ref(),
        PlayerSkin::Unavailable(_) => None,
    }
}

/// Pointer identity avoids comparing large unchanged appearance payloads on every frame.
fn same_profile_allocation(a: &PlayerProfile, b: &PlayerProfile) -> bool {
    let PlayerProfile {
        unique_id: a_id,
        username: a_name,
        verified: a_verified,
        skin: a_skin,
    } = a;
    let PlayerProfile {
        unique_id: b_id,
        username: b_name,
        verified: b_verified,
        skin: b_skin,
    } = b;
    if a_id != b_id || a_verified != b_verified || a_name != b_name {
        return false;
    }
    match (a_skin, b_skin) {
        (PlayerSkin::Unavailable(a), PlayerSkin::Unavailable(b)) => a == b,
        (PlayerSkin::Standard(a), PlayerSkin::Standard(b)) => {
            let protocol::StandardSkin {
                width: a_width,
                height: a_height,
                rgba8: a_pixels,
                cape: a_cape,
                geometry: a_geometry,
            } = a;
            let protocol::StandardSkin {
                width: b_width,
                height: b_height,
                rgba8: b_pixels,
                cape: b_cape,
                geometry: b_geometry,
            } = b;
            a_width == b_width
                && a_height == b_height
                && Arc::ptr_eq(a_pixels.pixels(), b_pixels.pixels())
                && match (a_geometry, b_geometry) {
                    (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                    (None, None) => true,
                    _ => false,
                }
                && match (a_cape, b_cape) {
                    (Some(a), Some(b)) => {
                        let protocol::CapeImage {
                            width: a_width,
                            height: a_height,
                            rgba8: a_pixels,
                        } = a;
                        let protocol::CapeImage {
                            width: b_width,
                            height: b_height,
                            rgba8: b_pixels,
                        } = b;
                        a_width == b_width
                            && a_height == b_height
                            && Arc::ptr_eq(a_pixels, b_pixels)
                    }
                    (None, None) => true,
                    _ => false,
                }
        }
        _ => false,
    }
}
