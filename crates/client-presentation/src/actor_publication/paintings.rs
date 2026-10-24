//! Static hanging pictures share actor pages while retaining their dedicated ingress state.
use super::*;
use crate::presentation::actors::{ActorPresentationBatch, actor_is_render_candidate};
use assets::painting::{PAINTING_HALF_DEPTH, PAINTING_MOTIVES, motive};
use render_api::PoseGeneration;
use render_model::{ActorArtworkLocation, ActorArtworkPages, EntityRigId, RenderBoneTransform};
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
struct Part {
    rig: EntityRigId,
    artwork: ActorArtworkLocation,
}

/// Retains part routing and the shared rest pose across unchanged frames.
#[derive(Default)]
pub(super) struct PaintingRoutes {
    identity: Option<([u8; 32], usize, usize)>,
    parts: BTreeMap<&'static str, [Option<Part>; 2]>,
    bones: Option<Arc<[RenderBoneTransform]>>,
}

impl PaintingRoutes {
    /// Rebuilds only when the accepted entity or artwork snapshot changes.
    pub(super) fn refresh(
        &mut self,
        base: Option<&assets::RuntimeEntityAssets>,
        pack: Option<&assets::SessionEntityPack>,
        artwork: &ActorArtworkPages,
    ) {
        let key = (
            artwork.identity,
            base.map_or(0, |base| std::ptr::from_ref(base) as usize),
            pack.map_or(0, |pack| Arc::as_ptr(&pack.assets) as usize),
        );
        if self.identity == Some(key) {
            return;
        }
        self.identity = Some(key);
        self.parts.clear();
        self.bones = Some(Arc::from([RenderBoneTransform {
            rotation: [0.0, 0.0, 0.0, 1.0],
            translation_scale: [0.0, 0.0, 0.0, 1.0],
            axis_scale: render_model::UNIT_AXIS_SCALE,
        }]));
        for motive in PAINTING_MOTIVES {
            let parts = [false, true].map(|border| {
                let identifier = motive.part_identifier(border);
                pack.and_then(|pack| part(&pack.assets, &identifier, true, artwork))
                    .or_else(|| base.and_then(|base| part(base, &identifier, false, artwork)))
            });
            self.parts.insert(motive.title, parts);
        }
    }

    /// Publishes only genuine painting ingress, after distance and frustum admission.
    pub(super) fn append(
        &self,
        world: &client_world::WorldAuthority,
        partial_tick: f32,
        view: Option<ActorCullView>,
        output: &mut ActorPresentationBatch,
    ) {
        for actor in world.painting_actors() {
            let Some(painting) = &actor.painting else {
                continue;
            };
            if actor.is_invisible() || !actor_is_render_candidate(actor, partial_tick, view) {
                continue;
            }
            let motive = motive(&painting.motive);
            let [width, height] = motive.size.map(|side| f32::from(side) / 16.0);
            let bounds = assets::SkinGeometryBounds {
                center: [0.0; 3],
                half_extents: [width / 2.0, height / 2.0, width / 2.0],
            };
            let world_half_extents = match painting.direction {
                protocol::PaintingDirection::South | protocol::PaintingDirection::North => {
                    [width / 2.0, height / 2.0, PAINTING_HALF_DEPTH]
                }
                protocol::PaintingDirection::West | protocol::PaintingDirection::East => {
                    [PAINTING_HALF_DEPTH, height / 2.0, width / 2.0]
                }
            };
            if !render_model::actor_bounds_are_visible(
                actor.position,
                1.0,
                assets::SkinGeometryBounds {
                    center: [0.0; 3],
                    half_extents: world_half_extents,
                },
                view,
            ) {
                continue;
            }
            let Some(parts) = self.parts.get(motive.title) else {
                continue;
            };
            let bones = self
                .bones
                .as_ref()
                .expect("painting routes have a rest pose");
            let yaw = painting.direction.yaw_degrees();
            let (sin, cos) = yaw.to_radians().sin_cos();
            let [x, y, z] = actor.position;
            let rows = [[cos, 0.0, sin, x], [0.0, 1.0, 0.0, y], [-sin, 0.0, cos, z]];
            for (layer, part) in parts.iter().enumerate() {
                let Some(part) = part else {
                    continue;
                };
                let submission = render_model::ActorRigSubmission {
                    material: Default::default(),
                    culling_bounds: bounds,
                    input: render_model::ActorRigRenderInput {
                        identity: render_model::ActorRenderIdentity {
                            session_id: world.actor_session_id(),
                            dimension: world.current_dimension(),
                            runtime_id: actor.runtime_id,
                            spawn_revision: actor.render_spawn_revision,
                            ingress_sequence: actor.spawn_revision.max(actor.movement_revision),
                            source_tick: actor.source_tick,
                            movement_revision: actor.movement_revision,
                            pose_generation: PoseGeneration::new(actor.spawn_revision),
                            layer: layer as u8,
                        },
                        rig: part.rig,
                        previous_bones: Arc::clone(bones),
                        current_bones: Arc::clone(bones),
                        completed_tick: actor.spawn_revision,
                        reset_generation: actor.spawn_revision,
                    },
                    world_from_actor: rows,
                    texture_layer: part.artwork.layer,
                    route: render_model::ActorRigRoute::StaticFallback,
                    tint: 0,
                    overlay_rgba8: 0,
                    uv_anim: render_model::IDENTITY_UV_ANIM,
                    light: render_model::pack_actor_light(0, 15),
                };
                output
                    .artwork
                    .insert(submission.input.identity, part.artwork);
                output.submissions.push(submission);
            }
        }
    }
}

/// Resolves a compiled part's geometry and raster within its owning index space.
fn part(
    assets: &assets::RuntimeEntityAssets,
    identifier: &str,
    pack: bool,
    artwork: &ActorArtworkPages,
) -> Option<Part> {
    let symbol = assets.symbols().iter().position(|symbol| {
        symbol.kind == assets::EntityAssetKind::Entity && symbol.identifier.as_ref() == identifier
    })?;
    let binding = assets
        .rig_bindings()
        .iter()
        .find(|binding| binding.entity_symbol as usize == symbol)?;
    let index = binding.first_geometry;
    let rig = if pack {
        render_model::pack_rig_id(index)
    } else {
        EntityRigId(index)
    };
    Some(Part {
        rig,
        artwork: artwork.route(rig)?,
    })
}

#[cfg(test)]
#[path = "paintings/tests.rs"]
mod tests;
