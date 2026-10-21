//! World actor flames use authoritative fire metadata and the admitted collision box.

use std::{collections::HashMap, sync::Arc};

use bevy::prelude::Resource;
use render_model::{
    ACTOR_LAYER_FLAME as FLAME_LAYER, ActorArtworkLocation, ActorArtworkPages, ActorMaterial,
    ActorRigGeometry, ActorRigRenderInput, ActorRigRoute, ActorRigSubmission, EquipmentRaster,
    RenderBoneTransform,
    actor::flame::{FLAME_RIG_ID, FLAME_SCALE, FlamePlayback, flame_depth, flame_geometry},
};

use crate::{
    local_player::FrozenLocalAvatarVisibility,
    presentation::actors::{ActorPresentationBatch, ActorRigPresentation},
};

const _: () = assert!(FLAME_LAYER < render_model::hats::ACTOR_LAYER_HAT);

type ActorKey = (u64, i32, u64, u64);

#[cfg(test)]
mod cosmetic_layer_tests {
    #[test]
    fn cosmetic_layers_do_not_alias_skin_flame_or_controller_layers() {
        for slot in 0..cosmetics::SLOT_COUNT {
            let layer = render_model::hats::ACTOR_LAYER_HAT + slot as u8;
            assert!(!crate::presentation::skin_layers::is_skin_layer(layer));
            assert_ne!(layer, super::FLAME_LAYER);
            assert!(layer < crate::presentation::entity_layers::ACTOR_LAYER_TEXTURE_BASE);
            assert!(layer > crate::presentation::cape::ACTOR_LAYER_CAPE);
        }
    }
}

/// Physical local origin bound to the exact selected body, independent of Java's model lift.
#[derive(Clone, Copy)]
pub(super) struct LocalFlameFeet {
    identity: render_model::ActorRenderIdentity,
    feet: [f32; 3],
}

impl LocalFlameFeet {
    /// Captures the same frozen physics sample used to place this frame's local body.
    pub(super) fn capture(
        local: Option<&ActorRigPresentation>,
        visibility: Option<FrozenLocalAvatarVisibility>,
    ) -> Option<Self> {
        let identity = local?.submission.input.identity;
        let visibility = visibility.filter(|visibility| {
            visibility.visible() && visibility.runtime_id() == identity.runtime_id
        })?;
        Some(Self {
            identity,
            feet: visibility.feet().to_array(),
        })
    }
}

/// Immutable flame artwork and per-spawn playback, retained across world frames.
#[derive(Resource, Default)]
pub struct WorldActorFlames {
    frames: Vec<ActorArtworkLocation>,
    geometry: Option<ActorRigGeometry>,
    bones: Arc<[RenderBoneTransform]>,
    playback: HashMap<ActorKey, FlamePlayback>,
}

impl WorldActorFlames {
    /// Installs original square animation frames once; absent or invalid optional art draws nothing.
    pub fn prepare(
        pages: ActorArtworkPages,
        texture: Option<&assets::ParticleTexture>,
    ) -> (Self, ActorArtworkPages) {
        let Some(texture) = texture.filter(|texture| {
            texture.path.as_ref() == assets::ACTOR_FLAME_TEXTURE
                && texture.width > 0
                && texture.height > 0
                && texture.height.is_multiple_of(texture.width)
                && u16::try_from(texture.width).is_ok()
                && texture
                    .width
                    .checked_mul(texture.height)
                    .and_then(|size| size.checked_mul(4))
                    == u32::try_from(texture.rgba8.len()).ok()
        }) else {
            return (Self::default(), pages);
        };
        let side = texture.width as u16;
        let rasters: Vec<_> = texture
            .rgba8
            .chunks_exact(usize::from(side).pow(2) * 4)
            .map(|pixels| EquipmentRaster {
                width: side,
                height: side,
                rgba8: pixels.into(),
            })
            .collect();
        let (pages, locations) = pages.with_equipment_rasters(&rasters);
        if locations.iter().any(Option::is_none) {
            return (Self::default(), pages);
        }
        (
            Self {
                frames: locations.into_iter().flatten().collect(),
                geometry: Some(flame_geometry(side)),
                bones: Arc::from([RenderBoneTransform {
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    translation_scale: [0.0, 0.0, 0.0, 1.0],
                    axis_scale: render_model::UNIT_AXIS_SCALE,
                }]),
                playback: HashMap::new(),
            },
            pages,
        )
    }

    /// Shares the startup mesh with the actor geometry catalog.
    pub fn geometry(&self) -> Option<ActorRigGeometry> {
        self.geometry.clone()
    }

    /// Adds flames only to selected world bodies, after local first-person and invisibility gating.
    pub(super) fn append(
        &mut self,
        world: Option<&client_world::WorldAuthority>,
        camera_yaw: f32,
        partial_tick: f32,
        local: Option<LocalFlameFeet>,
        batch: &mut ActorPresentationBatch,
    ) {
        let Some(world) = world else {
            self.playback.clear();
            return;
        };
        self.playback
            .retain(|&(session, dimension, runtime, spawn), _| {
                session == world.actor_session_id()
                    && dimension == world.current_dimension()
                    && world
                        .actor(runtime)
                        .is_some_and(|actor| actor.spawn_revision == spawn)
            });
        if self.frames.is_empty() || !camera_yaw.is_finite() {
            return;
        }
        let bodies = batch.submissions.len();
        for index in 0..bodies {
            let body = &batch.submissions[index];
            if body.input.identity.layer != render_model::ACTOR_LAYER_BODY
                || body.route == ActorRigRoute::NoDraw
            {
                continue;
            }
            let Some(actor) = world.actor(body.input.identity.runtime_id) else {
                continue;
            };
            if !actor.is_on_fire() || actor.is_invisible() {
                continue;
            }
            let Some((low, high)) = actor.bounding_box_at([0.0; 3]) else {
                continue;
            };
            let Some(feet) = local
                .filter(|local| local.identity == body.input.identity)
                .map(|local| local.feet)
                .or_else(|| actor.interpolated_position(partial_tick))
            else {
                continue;
            };
            let (width, height) = (high[0] - low[0], high[1] - low[1]);
            let mut identity = body.input.identity;
            let key = (
                identity.session_id,
                identity.dimension,
                identity.runtime_id,
                identity.spawn_revision,
            );
            let playback = self.playback.entry(key).or_default();
            playback.advance(true, self.frames.len());
            let artwork = self.frames[playback.frame()];
            identity.layer = FLAME_LAYER;
            let horizontal = width * FLAME_SCALE;
            let vertical = width.min(height) * FLAME_SCALE;
            let depth = flame_depth(width, height) * horizontal;
            // The mesh faces -Z; turn it toward the viewer so its negative depth
            // offset brings flames in front of the body rather than behind it.
            let (sin, cos) = (camera_yaw + std::f32::consts::PI).sin_cos();
            let [x, y, z] = feet;
            let submission = ActorRigSubmission {
                input: ActorRigRenderInput {
                    identity,
                    rig: FLAME_RIG_ID,
                    previous_bones: Arc::clone(&self.bones),
                    current_bones: Arc::clone(&self.bones),
                    completed_tick: body.input.completed_tick,
                    reset_generation: body.input.reset_generation,
                },
                world_from_actor: [
                    [cos * horizontal, 0.0, sin * horizontal, x + sin * depth],
                    [0.0, vertical, 0.0, y],
                    [-sin * horizontal, 0.0, cos * horizontal, z + cos * depth],
                ],
                material: ActorMaterial {
                    state: Some(assets::EntityRenderMaterialState {
                        alpha_test: true,
                        cull: true,
                        blend: true,
                        depth_write: true,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                culling_bounds: Default::default(),
                texture_layer: artwork.layer,
                route: ActorRigRoute::StaticFallback,
                tint: 0,
                overlay_rgba8: 0,
                uv_anim: render_model::IDENTITY_UV_ANIM,
                light: 0,
            };
            batch.artwork.insert(identity, artwork);
            batch.submissions.push(submission);
        }
    }
}

#[cfg(test)]
pub(super) mod tests;
