//! Per-tick pose conversions and presentations, reused by every frame of a tick.

use super::*;

/// Render-space poses and presentations of each rig's latest tick, kept across frames: every
/// frame of a tick shares one conversion and presentation, re-placing only its transform, and an
/// unchanged pose keeps its allocation so its bone matrices are reused.
#[derive(Debug, Default)]
pub struct PoseConversions {
    entries: std::collections::HashMap<u64, PoseEntry>,
    presentations: std::collections::HashMap<u64, PresentationEntry>,
    scratch: Vec<RenderBoneTransform>,
    frame: u64,
    builds: u64,
}

#[derive(Debug)]
struct PresentationEntry {
    key: TickKey,
    presentation: Option<TickPresentation>,
    used: u64,
}

/// Every input a [`TickPresentation`] reads; any difference rebuilds it.
#[derive(Debug, PartialEq)]
pub(super) struct TickKey {
    lifetime: client_world::ActorLifetimeId,
    rig: u32,
    completed_tick: u64,
    reset_generation: u64,
    /// Address and length of the previous, current and rest poses.
    poses: [(usize, usize); 3],
    rest_tick: (u64, u64),
    fallback: EntityRigFallback,
    bounds: assets::SkinGeometryBounds,
    scale: u32,
    actor: (u64, u64, i64),
    skin: SkinKey,
    location: Option<ActorArtworkLocation>,
}

impl TickKey {
    /// `selected` is the pose drawn; `rig` supplies the rest pose a rest-mode rig validates.
    pub(super) fn new(
        selected: &ActorRigSnapshot<'_>,
        rig: &ActorRigSnapshot<'_>,
        actor: &ActorSnapshot,
        profile: Option<&PlayerProfile>,
        location: Option<ActorArtworkLocation>,
    ) -> Self {
        let pose = |bones: &[client_world::BoneTransform]| (bones.as_ptr() as usize, bones.len());
        Self {
            lifetime: selected.actor,
            rig: selected.rig.0,
            completed_tick: selected.completed_tick,
            reset_generation: selected.reset_generation,
            poses: [
                pose(selected.previous),
                pose(selected.current),
                pose(rig.rest),
            ],
            rest_tick: (rig.rest_completed_tick, rig.rest_reset_generation),
            fallback: selected.fallback,
            bounds: selected.culling_bounds(),
            scale: selected.scale.to_bits(),
            actor: (actor.runtime_id, actor.spawn_revision, actor.unique_id),
            skin: SkinKey::new(actor, profile),
            location,
        }
    }
}

/// The profile skin a player's presentation draws, by allocation.
#[derive(Debug)]
enum SkinKey {
    NotPlayer,
    Default,
    Standard(Arc<[u8]>, u32, u32),
}

impl SkinKey {
    fn new(actor: &ActorSnapshot, profile: Option<&PlayerProfile>) -> Self {
        if !matches!(actor.kind, ActorKind::Player { .. }) {
            return Self::NotPlayer;
        }
        match profile
            .filter(|profile| profile.unique_id == actor.unique_id)
            .map(|profile| &profile.skin)
        {
            Some(PlayerSkin::Standard(skin)) => {
                Self::Standard(Arc::clone(skin.rgba8.pixels()), skin.width, skin.height)
            }
            Some(PlayerSkin::Unavailable(_)) | None => Self::Default,
        }
    }
}

impl PartialEq for SkinKey {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::NotPlayer, Self::NotPlayer) | (Self::Default, Self::Default) => true,
            (
                Self::Standard(left, left_width, left_height),
                Self::Standard(right, width, height),
            ) => Arc::ptr_eq(left, right) && (left_width, left_height) == (width, height),
            _ => false,
        }
    }
}

#[derive(Debug)]
struct PoseEntry {
    /// Spawn revision, completed tick, reset generation and pose storage of the conversion.
    stamp: (u64, u64, u64, usize, usize),
    previous: Arc<[RenderBoneTransform]>,
    current: Arc<[RenderBoneTransform]>,
    used: u64,
}

type RenderPose = Arc<[RenderBoneTransform]>;

/// Frames a rig may go undrawn before its conversions are released.
const POSE_RETENTION_FRAMES: u64 = 120;

impl PoseConversions {
    pub fn begin_frame(&mut self) {
        self.frame += 1;
        let frame = self.frame;
        self.entries
            .retain(|_, entry| entry.used + POSE_RETENTION_FRAMES >= frame);
        self.presentations
            .retain(|_, entry| entry.used + POSE_RETENTION_FRAMES >= frame);
    }

    /// Tick presentations built rather than reused.
    #[cfg(any(test, feature = "test-support"))]
    pub const fn presentation_builds(&self) -> u64 {
        self.builds
    }

    pub(super) fn tick_presentation(
        &mut self,
        key: TickKey,
        build: impl FnOnce(&mut Self) -> Option<TickPresentation>,
    ) -> Option<TickPresentation> {
        let (frame, runtime_id) = (self.frame, key.lifetime.runtime_id);
        if let Some(entry) = self.presentations.get_mut(&runtime_id)
            && entry.key == key
        {
            entry.used = frame;
            if let Some(pose) = self.entries.get_mut(&runtime_id) {
                pose.used = frame;
            }
            return entry.presentation.clone();
        }
        self.builds += 1;
        let presentation = build(self);
        self.presentations.insert(
            runtime_id,
            PresentationEntry {
                key,
                presentation: presentation.clone(),
                used: frame,
            },
        );
        presentation
    }

    pub(super) fn convert(
        &mut self,
        rig: &ActorRigSnapshot<'_>,
    ) -> Option<(RenderPose, RenderPose)> {
        let stamp = (
            rig.actor.spawn_revision,
            rig.completed_tick,
            rig.reset_generation,
            rig.previous.as_ptr() as usize,
            rig.current.as_ptr() as usize,
        );
        let frame = self.frame;
        if let Some(entry) = self.entries.get_mut(&rig.actor.runtime_id)
            && entry.stamp == stamp
        {
            entry.used = frame;
            return Some((Arc::clone(&entry.previous), Arc::clone(&entry.current)));
        }
        let old = self
            .entries
            .get(&rig.actor.runtime_id)
            .map(|entry| (Arc::clone(&entry.previous), Arc::clone(&entry.current)));
        let mut reuse = |bones: &[client_world::BoneTransform]| {
            self.scratch.clear();
            for bone in bones {
                self.scratch
                    .push(RenderBoneTransform::from_model_space_scaled(
                        bone.rotation,
                        bone.translation_scale,
                        bone.axis_scale,
                    )?);
            }
            // The new tick's previous pose is usually the last tick's current one.
            Some(
                old.iter()
                    .flat_map(|(previous, current)| [current, previous])
                    .find(|pose| ***pose == *self.scratch)
                    .map_or_else(|| Arc::from(self.scratch.as_slice()), Arc::clone),
            )
        };
        let previous = reuse(rig.previous)?;
        let current = if rig.current == rig.previous {
            Arc::clone(&previous)
        } else {
            reuse(rig.current)?
        };
        self.entries.insert(
            rig.actor.runtime_id,
            PoseEntry {
                stamp,
                previous: Arc::clone(&previous),
                current: Arc::clone(&current),
                used: frame,
            },
        );
        Some((previous, current))
    }
}

pub(crate) fn convert_bones(
    bones: &[client_world::BoneTransform],
) -> Option<Arc<[RenderBoneTransform]>> {
    convert_bones_with(bones, &mut Vec::with_capacity(bones.len()))
}

/// [`convert_bones`] staging the conversion in `scratch`, so only the shared pose allocates.
pub(crate) fn convert_bones_with(
    bones: &[client_world::BoneTransform],
    scratch: &mut Vec<RenderBoneTransform>,
) -> Option<Arc<[RenderBoneTransform]>> {
    scratch.clear();
    for bone in bones {
        scratch.push(RenderBoneTransform::from_model_space_scaled(
            bone.rotation,
            bone.translation_scale,
            bone.axis_scale,
        )?);
    }
    Some(Arc::from(scratch.as_slice()))
}

#[cfg(test)]
mod tests;
