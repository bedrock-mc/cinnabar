//! Catalog attachments follow admitted actors and their current bone poses.
use super::RemoteHat;
use crate::presentation::actors::ActorPresentationBatch;
use bevy::prelude::Resource;
use cosmetics::Item;
use render_model::{
    ACTOR_LAYER_BODY, ActorEmitterTransform, ActorMaterial, ActorRigRoute, MAX_RENDERED_PLAYERS,
    hats::{self, HatPoseCache},
};
use std::{collections::HashMap, sync::Arc};

/// Identifies one authored emitter of one equipped item on one actor lifetime.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CosmeticEmitterKey {
    pub runtime_id: u64,
    pub spawn_revision: u64,
    pub item: Item,
    pub emitter: u8,
}

/// A world-visible cosmetic emitter placed on this frame's posed attachment.
#[derive(Clone, Debug)]
pub struct CosmeticEmitterPlacement {
    pub key: CosmeticEmitterKey,
    /// Client-owned effect identifier; register `effect` under it before spawning.
    pub identifier: Arc<str>,
    pub effect: Arc<[u8]>,
    /// Particle-atlas tile key and `COSMETIC_SPRITE_SIDE`² RGBA8 texels for `atlas.cosmetic`
    /// effects; `None` when the effect samples a runtime atlas path.
    pub sprite: Option<(u64, Arc<[u8]>)>,
    pub transform: ActorEmitterTransform,
}

/// Emitters of every cosmetic the latest actor frame draws; absent items have no emitters.
#[derive(Resource, Default)]
pub struct CosmeticEmitterPlacements(pub Vec<CosmeticEmitterPlacement>);

/// Geometry and render-only skeletons sampled for the current attachment frame.
#[derive(Default)]
pub(super) struct AttachmentFrame<'a> {
    pub collisions: Option<&'a dyn crate::observations::CollisionLookup>,
    pub hidden_local: Option<&'a render_model::ActorRigSubmission>,
    pub geometries: &'a [render_model::ActorRigGeometry],
    pub equipment: Option<&'a crate::presentation::equipment::EquipmentRuntime>,
    pub local_emote: Option<&'a actor_animation::CustomEmotePose>,
    pub remote_emotes:
        Option<&'a bevy::platform::collections::HashMap<u64, actor_animation::CustomEmotePose>>,
}

impl<'a> AttachmentFrame<'a> {
    /// Uses the same articulated skeleton as the submitted body during custom playback.
    pub(super) fn rig(
        &'a self,
        rig: actor_animation::ActorRigSnapshot<'a>,
        local: bool,
    ) -> actor_animation::ActorRigSnapshot<'a> {
        let emote = if local {
            self.local_emote
        } else {
            self.remote_emotes
                .and_then(|poses| poses.get(&rig.actor.runtime_id))
        };
        emote.map_or(rig, |pose| pose.snapshot(rig))
    }
}

/// Bedrock's emissive-alpha state for fullbright catalog items. Other material fields, culling
/// and overlay suppression follow the body, so player cosmetics stay two-sided as before.
fn emissive_material(body: ActorMaterial) -> ActorMaterial {
    ActorMaterial {
        state: Some(assets::EntityRenderMaterialState {
            alpha_test: true,
            cull: body.state.is_some_and(|state| state.cull),
            blend: false,
            depth_write: true,
            emissive: true,
            disable_overlay: body.state.is_some_and(|state| state.disable_overlay),
            ..Default::default()
        }),
        ..body
    }
}

#[derive(Default)]
struct Attachment {
    companion: super::companions::State,
    cloth: hats::cloth::ClothState,
    contacts: Option<Vec<hats::cloth::Collider>>,
    wind: hats::EarWind,
    motion: hats::Motion,
    poses: [HatPoseCache; 2],
    seen: u64,
    visible: Option<bool>,
    pose_reset: Option<u64>,
}

impl Attachment {
    /// Transfer cloth history between the hand-context body and the visible world body on F5.
    fn set_visible(&mut self, visible: bool) {
        if self.visible.is_some_and(|old| old != visible) {
            self.cloth.reanchor();
        }
        self.visible = Some(visible);
    }

    /// Samples attachment bones and advances fabric against the owning avatar's body proxies.
    fn pose(
        &mut self,
        item: Item,
        body: &render_model::ActorRigSubmission,
        (names, bindings): (
            &[Box<str>],
            Option<&Arc<render_model::ActorAttachmentBindings>>,
        ),
        timing: (f32, f32),
    ) -> Option<hats::cloth::ClothPose> {
        let bone = self.poses[1].anchor(item, bindings, names)?;
        let previous = *body.input.previous_bones.get(bone)?;
        let current = *body.input.current_bones.get(bone)?;
        let current_pose = self.poses[1].pose(item, current, self.motion);
        let previous_pose = if previous == current {
            Arc::clone(&current_pose)
        } else {
            self.poses[0].pose(item, previous, self.motion)
        };
        if !hats::rig(item).has_cloth() {
            return Some((previous_pose, current_pose));
        }
        if self
            .pose_reset
            .replace(body.input.reset_generation)
            .is_some_and(|old| old != body.input.reset_generation)
        {
            // The authority's first-person context can arrive after the visibility change.
            self.cloth.reanchor();
        }
        if self.visible == Some(false) && self.contacts.is_none() {
            self.cloth.retain_fitting();
        }
        let colliders: Vec<_> = self.contacts.take().unwrap_or_else(|| {
            names
                .iter()
                .zip(body.input.current_bones.iter())
                .filter_map(|(name, bone)| {
                    hats::cloth::Collider::biped(
                        name,
                        *bone,
                        hats::cloth::placement(body.world_from_actor),
                    )
                })
                .collect()
        });
        let bases = [previous_pose, current_pose];
        Some(
            self.cloth
                .pose(
                    item,
                    bases.clone(),
                    body.world_from_actor,
                    timing,
                    &colliders,
                )
                .unwrap_or_else(|| {
                    let [previous, current] = bases;
                    (previous, current)
                }),
        )
    }
}
#[derive(Default)]
pub(super) struct HatState {
    attachments: HashMap<(u64, u64, Item), Attachment>,
    local: Vec<Item>,
    local_attachments: HashMap<Item, Attachment>,
    /// Submission indices of this frame's attachments, for emitter placement.
    drawn: Vec<(usize, Item)>,
    seconds: f32,
    frame: u64,
}
impl HatState {
    /// Whether hidden local cloth needs the wearer's current body and equipment contacts.
    pub(super) fn has_local_cloth(&self) -> bool {
        self.local.iter().any(|item| hats::rig(*item).has_cloth())
    }

    /// Captures local slot selections and caller-owned motion for this frame.
    pub(super) fn advance(&mut self, items: &[Item], speed: f32, seconds: f32) {
        self.local.clear();
        self.local.extend_from_slice(items);
        self.seconds = seconds;
        self.frame = self.frame.wrapping_add(1);
        self.local_attachments
            .retain(|item, _| items.contains(item));
        for &item in items {
            let attachment = self.local_attachments.entry(item).or_default();
            attachment.motion.bend = attachment.wind.advance(item, speed, seconds);
            attachment.motion.advance(item, seconds);
        }
    }

    /// Shares local motion with the HUD even while the world body is hidden.
    pub(super) fn local_motion(&self) -> Vec<(Item, hats::Motion)> {
        self.local
            .iter()
            .filter_map(|item| {
                self.local_attachments
                    .get(item)
                    .map(|attachment| (*item, attachment.motion))
            })
            .collect()
    }

    /// Keeps equipped fabric current while first-person visibility omits the local body draw.
    pub(super) fn advance_hidden_local(
        &mut self,
        body: &render_model::ActorRigSubmission,
        names: &[Box<str>],
        bindings: Option<&Arc<render_model::ActorAttachmentBindings>>,
        partial_tick: f32,
        contacts: Option<&[hats::cloth::Collider]>,
    ) {
        for item in &self.local {
            if hats::rig(*item).has_cloth() {
                let attachment = self.local_attachments.entry(*item).or_default();
                attachment.set_visible(false);
                attachment.contacts = contacts.map(<[hats::cloth::Collider]>::to_vec);
                let _ =
                    attachment.pose(*item, body, (names, bindings), (self.seconds, partial_tick));
            }
        }
    }

    /// Submits each slot at its catalog bone, sharing immutable geometry and one texture per item.
    pub(super) fn attach(
        &mut self,
        batch: &mut ActorPresentationBatch,
        stream: Option<&chunk_pipeline::WorldStream>,
        items: &[RemoteHat],
        scene: &mut render_scene::ActorRenderScene,
        partial_tick: f32,
        frame: AttachmentFrame<'_>,
    ) {
        self.drawn.clear();
        let Some(stream) = stream else {
            self.attachments.clear();
            return;
        };
        let mut textures = HashMap::new();
        let mut bodies: Vec<_> = batch
            .submissions
            .iter()
            .map(|body| (body.clone(), false))
            .collect();
        if let Some(body) = frame.hidden_local {
            bodies.push((body.clone(), true));
        }
        for (body, hidden) in bodies {
            let id = body.input.identity.runtime_id;
            if body.input.identity.layer != ACTOR_LAYER_BODY || body.route == ActorRigRoute::NoDraw
            {
                continue;
            }
            let Some(actor) = stream.authority().actor(id) else {
                continue;
            };
            let local = id == stream.local_player_runtime_id();
            let selected: Vec<_> = if local {
                self.local.clone()
            } else {
                items
                    .iter()
                    .filter(|item| {
                        item.runtime_id == id && item.spawn_revision == actor.spawn_revision
                    })
                    .map(|item| item.hat)
                    .collect()
            };
            for item in selected {
                if hidden && hats::rig(item).companion().is_none() {
                    continue;
                }
                let Some(rig) = stream.authority().actor_rig(id) else {
                    continue;
                };
                let rig = frame.rig(rig, local);
                let attachment = if local {
                    let attachment = self.local_attachments.entry(item).or_default();
                    attachment.set_visible(true);
                    attachment
                } else {
                    let attachment = self
                        .attachments
                        .entry((id, actor.spawn_revision, item))
                        .or_default();
                    let speed = if actor.interpolation_ticks_remaining > 0 {
                        hats::forward_speed(actor.velocity, actor.head_yaw)
                            / simulation_clock::TICKS_PER_SECOND as f32
                    } else {
                        0.0
                    };
                    attachment.motion.bend = attachment.wind.advance(item, speed, self.seconds);
                    attachment.motion.advance(item, self.seconds);
                    attachment.seen = self.frame;
                    attachment
                };
                if hats::rig(item).has_cloth() {
                    attachment.contacts = Some(super::cloth_contacts::collect(
                        &body,
                        &rig,
                        &batch.submissions,
                        scene,
                        partial_tick,
                        (frame.geometries, frame.equipment),
                    ));
                }
                let companion = if hats::rig(item).companion().is_some() {
                    let Some(collisions) = frame.collisions else {
                        continue;
                    };
                    let Some(frame) = attachment.companion.advance(
                        item,
                        &body,
                        actor.head_yaw,
                        self.seconds,
                        stream,
                        collisions,
                    ) else {
                        continue;
                    };
                    Some(frame)
                } else {
                    None
                };
                let poses = if let Some(frame) = &companion {
                    Some((Arc::clone(&frame.1), Arc::clone(&frame.1)))
                } else {
                    attachment.pose(
                        item,
                        &body,
                        (rig.bone_names, rig.attachments),
                        (self.seconds, partial_tick),
                    )
                };
                let Some((previous, current)) = poses else {
                    continue;
                };
                let model = hats::rig(item);
                let texture_layer = if let Some(layer) = textures.get(&item) {
                    *layer
                } else {
                    if batch.skin_layers.len() >= MAX_RENDERED_PLAYERS {
                        continue;
                    }
                    let layer = batch.skin_layers.len() as u32;
                    batch.skin_layers.push(model.world_pixels().clone());
                    textures.insert(item, layer);
                    layer
                };
                if !scene.contains_geometry(model.geometry.id) {
                    let _ = scene.insert_geometry(model.geometry.clone());
                }
                let mut submission = body.clone();
                // The catalog slot index keeps layers unique for multiple attachments on one actor.
                if let Some((placement, _)) = &companion {
                    submission.world_from_actor = *placement;
                    submission.light = super::companions::lighting(*placement, |position| {
                        stream.solved_light_at(position)
                    });
                } else if model.has_cloth() {
                    submission.world_from_actor = attachment
                        .cloth
                        .fitted_placement()
                        .unwrap_or(body.world_from_actor);
                }
                submission.input.identity.layer = hats::ACTOR_LAYER_HAT
                    + cosmetics::slot_index(&item.descriptor().slot).unwrap_or(0) as u8;
                submission.input.rig = model.geometry.id;
                submission.input.previous_bones = previous;
                submission.input.current_bones = current;
                submission.texture_layer = texture_layer;
                submission.tint = 0;
                if model.emissive() {
                    submission.material = emissive_material(body.material);
                }
                self.drawn.push((batch.submissions.len(), item));
                batch.submissions.push(submission);
            }
        }
        self.attachments
            .retain(|_, attachment| attachment.seen == self.frame);
    }

    /// Identifies independently placed companions that remain visible beside a coverage-only owner.
    pub(super) fn independent_layers(&self, batch: &ActorPresentationBatch) -> Vec<(u64, u8)> {
        self.drawn
            .iter()
            .filter_map(|&(index, item)| {
                hats::rig(item).companion()?;
                let identity = batch.submissions.get(index)?.input.identity;
                Some((identity.runtime_id, identity.layer))
            })
            .collect()
    }

    /// Replaces `placements` with the emitters of this frame's attachments that still draw after
    /// visibility styling, sampled at the frame's actor interpolation like their vertices.
    pub(super) fn place_emitters(
        &self,
        batch: &ActorPresentationBatch,
        partial_tick: f32,
        placements: &mut Vec<CosmeticEmitterPlacement>,
    ) {
        placements.clear();
        for &(index, item) in &self.drawn {
            let Some(submission) = batch.submissions.get(index) else {
                continue;
            };
            if submission.route == ActorRigRoute::NoDraw {
                continue;
            }
            let model = hats::rig(item);
            for (emitter, definition) in model.emitters().iter().enumerate() {
                let Some(transform) = model.emitter_transform(
                    emitter,
                    &submission.input.previous_bones,
                    &submission.input.current_bones,
                    submission.world_from_actor,
                    partial_tick,
                ) else {
                    continue;
                };
                placements.push(CosmeticEmitterPlacement {
                    key: CosmeticEmitterKey {
                        runtime_id: submission.input.identity.runtime_id,
                        spawn_revision: submission.input.identity.spawn_revision,
                        item,
                        emitter: emitter as u8,
                    },
                    identifier: Arc::clone(&definition.identifier),
                    effect: Arc::clone(&definition.effect),
                    sprite: definition
                        .sprite
                        .as_ref()
                        .map(|sprite| (sprite.key, Arc::clone(&sprite.pixels))),
                    transform,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_motion_advances_without_a_visible_body_and_unequip_retires_it() {
        let item = render_model::hats::fixture::bunny();
        let rigid = render_model::hats::fixture::rigid();
        let wings = render_model::hats::fixture::dragon_wings();
        let mut state = HatState::default();
        state.advance(&[item, rigid, wings], 6.0, 1.0);
        let bends = state.local_motion();
        assert!(
            bends
                .iter()
                .any(|(key, bend)| *key == item && bend.bend > 0.5)
        );
        assert!(bends.contains(&(rigid, hats::Motion::default())));
        assert!(
            bends
                .iter()
                .any(|(key, motion)| *key == wings && motion.seconds == 1.0)
        );
        state.advance(&[], 0.0, 1.0);
        assert!(state.local_motion().is_empty());
    }
    #[test]
    fn pet_arrival_restarts_only_after_unequipping() {
        let pet = render_model::hats::fixture::shoulder_parrot();
        let hat = render_model::hats::fixture::bunny();
        let mut state = HatState::default();
        state.advance(&[pet], 0.0, 0.5);
        state.advance(&[pet, hat], 0.0, 0.5);
        assert_eq!(
            state
                .local_motion()
                .into_iter()
                .find(|(item, _)| *item == pet)
                .unwrap()
                .1
                .seconds,
            1.0
        );
        state.advance(&[hat], 0.0, 0.5);
        state.advance(&[pet, hat], 0.0, 0.1);
        assert_eq!(
            state
                .local_motion()
                .into_iter()
                .find(|(item, _)| *item == pet)
                .unwrap()
                .1
                .seconds,
            0.1
        );
    }

    #[test]
    fn hidden_armored_cloth_reveals_without_waiting_for_a_fixed_step() {
        use bevy::math::{Mat4, Vec3};
        let item = render_model::hats::fixture::cloth();
        let root = render_model::RenderBoneTransform {
            rotation: bevy::math::Quat::IDENTITY.to_array(),
            translation_scale: [0.0, 1.5, 0.0, 1.0],
            axis_scale: render_model::UNIT_AXIS_SCALE,
        };
        let armor = render_model::ActorRigGeometry::synthetic_cuboid(
            render_model::EntityRigId(1),
            [-0.25, -0.75, -3.0 / 16.0],
            [0.25, 0.0, 3.0 / 16.0],
            1,
        )
        .unwrap();
        let mut body = crate::presentation::actors::local_diagnostic_presentation(
            1,
            0,
            1,
            render_api::PoseGeneration::new(1),
            [0.0; 3],
            0.0,
            0.0,
        )
        .unwrap()
        .submission;
        body.input.previous_bones = vec![root].into();
        body.input.current_bones = vec![root].into();
        let names = vec![Box::<str>::from("body")];
        let mut state = HatState::default();
        for frame in 0..240 {
            body.world_from_actor[2][3] = frame as f32 / 60.0;
            let world = hats::cloth::placement(body.world_from_actor);
            let contacts = [
                hats::cloth::Collider::biped("body", root, world).unwrap(),
                hats::cloth::Collider::geometry("body", &armor, 0, root, world).unwrap(),
            ];
            state.advance(&[item], 0.0, 1.0 / 120.0);
            state.advance_hidden_local(&body, &names, None, 0.5, Some(&contacts));
        }
        let world = hats::cloth::placement(body.world_from_actor);
        let attachment = state.local_attachments.get_mut(&item).unwrap();
        attachment.contacts = Some(vec![
            hats::cloth::Collider::geometry("body", &armor, 0, root, world).unwrap(),
        ]);
        attachment.set_visible(true);
        let (_, pose) = attachment
            .pose(item, &body, (&names, None), (0.0, 0.5))
            .unwrap();
        let placed = hats::cloth::placement(attachment.cloth.fitted_placement().unwrap());
        let local_from_world: Mat4 = world.inverse();
        let mut checked = 0;
        // The synthetic surface owns one particle bone per distinct rest point.
        for particle in pose.iter().skip(2) {
            let point = local_from_world.transform_point3(
                placed.transform_point3(Vec3::from_slice(&particle.translation_scale[..3])),
            );
            if point.x.abs() < 0.25 && point.y > 0.75 && point.y < 1.5 {
                assert!(
                    point.z > 3.0 / 16.0,
                    "revealed fabric entered the chestplate: {point}"
                );
                checked += 1;
            }
        }
        assert!(
            checked > 0,
            "fixture must exercise cloth over the chestplate"
        );
    }

    #[test]
    fn hidden_local_cloth_reappears_at_the_continuously_simulated_pose() {
        let item = render_model::hats::fixture::cloth();
        let root = render_model::RenderBoneTransform {
            rotation: bevy::math::Quat::IDENTITY.to_array(),
            translation_scale: [0.0, 1.5, 0.0, 1.0],
            axis_scale: render_model::UNIT_AXIS_SCALE,
        };
        let mut body = crate::presentation::actors::local_diagnostic_presentation(
            1,
            0,
            1,
            render_api::PoseGeneration::new(1),
            [0.0; 3],
            0.0,
            0.0,
        )
        .unwrap()
        .submission;
        body.input.previous_bones = vec![root].into();
        body.input.current_bones = vec![root].into();
        let names = vec![Box::<str>::from("body")];
        let mut hidden = HatState::default();
        let mut visible = Attachment::default();
        let mut frozen = Attachment::default();
        for frame in 0..240 {
            let seconds = frame as f32 / 120.0;
            body.world_from_actor[1][3] = (seconds * 3.0).sin().max(0.0) * 0.8;
            body.world_from_actor[2][3] = seconds * 2.0;
            hidden.advance(&[item], 0.0, 1.0 / 120.0);
            let expected = visible
                .pose(item, &body, (&names, None), (1.0 / 120.0, 0.5))
                .unwrap();
            if frame == 0 {
                let _ = frozen.pose(item, &body, (&names, None), (1.0 / 120.0, 0.5));
            }
            if frame < 239 {
                hidden.advance_hidden_local(&body, &names, None, 0.5, None);
            } else {
                let revealed = hidden
                    .local_attachments
                    .get_mut(&item)
                    .unwrap()
                    .pose(item, &body, (&names, None), (1.0 / 120.0, 0.5))
                    .unwrap();
                assert_eq!(
                    revealed, expected,
                    "perspective reveal must retain the moving cloth history"
                );
                let stale = frozen
                    .pose(item, &body, (&names, None), (1.0 / 120.0, 0.5))
                    .unwrap();
                assert_ne!(
                    stale, expected,
                    "the fixture must expose frozen first-person history"
                );
            }
        }
        hidden.advance(&[], 0.0, 1.0 / 120.0);
        assert!(
            hidden.local_attachments.is_empty(),
            "unequipping must retire hidden cloth too"
        );
    }

    #[test]
    fn perspective_changes_rebase_between_distinct_body_postures() {
        use bevy::math::{Mat4, Quat, Vec3};
        let item = render_model::hats::fixture::cloth();
        let root = render_model::RenderBoneTransform {
            rotation: Quat::IDENTITY.to_array(),
            translation_scale: [0.0, 1.5, 0.0, 1.0],
            axis_scale: render_model::UNIT_AXIS_SCALE,
        };
        let mut body = crate::presentation::actors::local_diagnostic_presentation(
            1,
            0,
            1,
            render_api::PoseGeneration::new(1),
            [0.0; 3],
            0.0,
            0.0,
        )
        .unwrap()
        .submission;
        body.input.previous_bones = vec![root].into();
        body.input.current_bones = vec![root].into();
        let names = vec![Box::<str>::from("body")];
        let mut attachment = Attachment::default();
        attachment.set_visible(false);
        let mut before = None;
        for _ in 0..120 {
            before = Some(
                attachment
                    .pose(item, &body, (&names, None), (1.0 / 120.0, 1.0))
                    .unwrap()
                    .1,
            );
        }
        let first = hats::rig(item)
            .geometry
            .vertices
            .iter()
            .map(|v| v.bone_index)
            .min()
            .unwrap() as usize;
        let source = Mat4::from_translation(Vec3::new(0.0, 1.5, 0.0));
        let local: Vec<_> = before.unwrap()[first..]
            .iter()
            .map(|bone| {
                source
                    .inverse()
                    .transform_point3(Vec3::from_slice(&bone.translation_scale[..3]))
            })
            .collect();
        for (visible, delayed_context) in [(true, false), (false, true), (true, true)] {
            attachment.set_visible(visible);
            if delayed_context {
                let _ = attachment
                    .pose(item, &body, (&names, None), (0.0, 1.0))
                    .unwrap();
            }
            let rotation = if visible {
                Quat::from_rotation_y(0.8) * Quat::from_rotation_x(0.45)
            } else {
                Quat::IDENTITY
            };
            let position = if visible {
                Vec3::new(0.0, 1.38, -0.06)
            } else {
                Vec3::new(0.0, 1.5, 0.0)
            };
            let target = render_model::RenderBoneTransform {
                rotation: rotation.to_array(),
                translation_scale: [position.x, position.y, position.z, 1.0],
                ..root
            };
            body.input.previous_bones = vec![target].into();
            body.input.current_bones = vec![target].into();
            body.input.reset_generation += 1;
            let pose = attachment
                .pose(item, &body, (&names, None), (0.0, 1.0))
                .unwrap()
                .1;
            let inverse = Mat4::from_rotation_translation(rotation, position).inverse();
            for (bone, expected) in pose[first..].iter().zip(&local) {
                let point =
                    inverse.transform_point3(Vec3::from_slice(&bone.translation_scale[..3]));
                assert!(
                    point.abs_diff_eq(*expected, 1e-5),
                    "perspective switch left cloth on the old posture: {point} vs {expected}"
                );
            }
        }
    }
}
