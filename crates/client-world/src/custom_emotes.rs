//! Original client-local emotes; no Marketplace/Lunar animation data is included.
use std::sync::Arc;

use crate::{ActorRigSnapshot, BoneTransform, RenderTextureLayer, SkinRenderLayer};

/// The single catalog identity shared by UI, persisted slots and playback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CustomEmote {
    Twerk,
}

impl CustomEmote {
    pub const ALL: &[Self] = &[Self::Twerk];
    pub const fn id(self) -> &'static str {
        match self {
            Self::Twerk => "cinnabar:twerk",
        }
    }
    pub const fn label(self) -> &'static str {
        match self {
            Self::Twerk => "Twerk",
        }
    }
    pub const fn duration_seconds(self) -> f64 {
        match self {
            Self::Twerk => 0.45,
        }
    }
    pub const fn looping(self) -> bool {
        true
    }
    pub const fn local_only(self) -> bool {
        true
    }
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|emote| emote.id() == id)
    }
}

/// Render-only poses owned by this sample, including clothing/persona geometries.
/// Feed `snapshot` into the ordinary *third-person* body publication path only.
/// Keep the native snapshot for the first-person hand and all remote actors.
#[derive(Clone, Debug)]
pub struct CustomEmotePose {
    pub previous: Arc<[BoneTransform]>,
    pub current: Arc<[BoneTransform]>,
    pub render: Vec<RenderTextureLayer>,
    pub skin_layers: Vec<SkinRenderLayer>,
    pub(crate) articulated: Option<CustomEmoteRig>,
}

#[derive(Clone, Debug)]
pub(crate) struct CustomEmoteRig {
    pub geometry: Arc<assets::SkinGeometry>,
    pub names: Arc<[Box<str>]>,
    pub rest: Arc<[BoneTransform]>,
}

impl CustomEmotePose {
    pub fn snapshot<'a>(&'a self, mut original: ActorRigSnapshot<'a>) -> ActorRigSnapshot<'a> {
        original.previous = &self.previous;
        original.current = &self.current;
        original.render = &self.render;
        original.skin_layers = &self.skin_layers;
        if let Some(rig) = &self.articulated {
            original.skin_geometry = Some(&rig.geometry);
            original.bone_names = &rig.names;
            original.rest = &rig.rest;
        }
        original
    }
}

/// Samples original authored channels before the native skeleton composition.
/// Pass the same render-time phase twice for an already-interpolated local body;
/// otherwise pass the two fixed-tick phases that the render alpha interpolates.
/// These allocations must bypass a conversion cache keyed only by pose pointers:
/// a dropped sample's address can be reused while its actor tick stays unchanged.
pub fn sample_custom_emote(
    rig: &ActorRigSnapshot<'_>,
    emote: CustomEmote,
    previous_seconds: f64,
    current_seconds: f64,
) -> Option<CustomEmotePose> {
    crate::actor_animation::custom_emotes::sample(rig, emote, previous_seconds, current_seconds)
}

#[cfg(test)]
mod tests {
    use super::CustomEmote;

    #[test]
    fn owned_custom_emote_catalog_has_round_trip_ids_and_loop_periods() {
        for emote in CustomEmote::ALL {
            assert_eq!(CustomEmote::from_id(emote.id()), Some(*emote));
            assert!(!emote.label().is_empty());
            assert!(emote.duration_seconds().is_finite() && emote.duration_seconds() > 0.0);
            assert!(emote.looping() && emote.local_only());
            assert_eq!(
                CustomEmote::ALL
                    .iter()
                    .filter(|entry| entry.id() == emote.id())
                    .count(),
                1
            );
        }
        assert_eq!(CustomEmote::from_id("unknown"), None);
    }
}
