//! Observed fighter and world-actor poses become native GPU submissions.
use std::{collections::HashMap, sync::Arc};

use bevy::math::Vec3;
use render::{
    ActorRenderFrame, ActorRenderIdentity, ActorRigRenderInput, ActorRigRoute, ActorRigSubmission,
};
use view_presentation::equipment_display;

use super::{AnimationView, BrowserActors, hash_id, parse_rgb};
use crate::browser_model::Frame;

impl BrowserActors {
    pub(crate) fn update(
        &mut self,
        current: &Frame,
        previous: Option<&Frame>,
        partial_tick: f32,
        view: AnimationView<'_>,
    ) -> ActorRenderFrame {
        self.generation = self.generation.wrapping_add(1).max(1);
        let hidden_player = view.hidden_player;
        let fraction = if partial_tick.is_finite() {
            partial_tick.clamp(0.0, 1.0)
        } else {
            1.0
        };
        let classic = Arc::clone(&self.classic.source);
        let slim_source = self.slim.as_ref().map(|rig| Arc::clone(&rig.source));
        let skins = &self.skins;
        let custom = &self.custom;
        let capes = &self.capes;
        self.animator
            .advance(current, previous, fraction, view, &self.equipment, |id| {
                let source = skins.get(id).map(|skin| {
                    if let Some(rig) = custom.get(id) {
                        return Arc::clone(&rig.source);
                    }
                    if skin.slim {
                        slim_source.as_ref().unwrap_or(&classic).clone()
                    } else {
                        classic.clone()
                    }
                });
                (source, capes.contains_key(id))
            });
        let animation_fraction = self.animator.partial_tick();
        let visible = current
            .fighters
            .iter()
            .filter(|fighter| !fighter.dead && hidden_player != Some(fighter.id.as_str()))
            .filter_map(|fighter| {
                let key = fighter.id.as_str();
                self.skins
                    .get(key)
                    .map(|skin| (fighter, key.to_owned(), skin.slim))
            })
            .collect::<Vec<_>>();
        let keys = visible
            .iter()
            .map(|(_, key, _)| key.clone())
            .collect::<Vec<_>>();
        if self.skin_keys != keys {
            self.render_skins = keys
                .iter()
                .map(|key| self.skins[key].pixels.clone())
                .collect();
            for key in &keys {
                if let Some(cape) = self.capes.get(key) {
                    self.render_skins.push(cape.clone());
                }
            }
            self.skin_keys = keys.clone();
        }
        let mut submissions = Vec::new();
        let mut persona_selections = Vec::new();
        let mut assignments = HashMap::new();
        for (layer, (fighter, _, slim)) in visible.iter().enumerate() {
            let rig = if let Some(rig) = self.custom.get(&fighter.id) {
                rig
            } else if *slim {
                self.slim.as_ref().unwrap_or(&self.classic)
            } else {
                &self.classic
            };
            let old = previous
                .and_then(|frame| {
                    frame
                        .fighters
                        .iter()
                        .find(|old| old.id == fighter.id && old.dead == fighter.dead)
                })
                .unwrap_or(fighter);
            let Some(animated) = self.animator.pose(&fighter.id, rig.id, &rig.names) else {
                continue;
            };
            let previous_bones = animated.previous;
            let current_bones = animated.current;
            let identity = ActorRenderIdentity {
                session_id: hash_id(&current.id),
                dimension: 0,
                runtime_id: hash_id(&fighter.id),
                spawn_revision: 1,
                ingress_sequence: self.generation,
                source_tick: Some(self.generation),
                movement_revision: self.generation,
                pose_generation: animated.completed_tick,
                layer: render::ACTOR_LAYER_BODY,
            };
            let yaw_delta =
                (animated.body_yaw - animated.previous_body_yaw + 180.0).rem_euclid(360.0) - 180.0;
            let position =
                Vec3::from_array(old.position).lerp(Vec3::from_array(fighter.position), fraction);
            let body = ActorRigSubmission {
                material: render::ActorMaterial::default(),
                culling_bounds: rig.bounds,
                input: ActorRigRenderInput {
                    identity,
                    rig: rig.id,
                    previous_bones,
                    current_bones,
                    completed_tick: animated.completed_tick,
                    reset_generation: animated.reset_generation,
                },
                world_from_actor: equipment_display::rig_world_from_actor(
                    position.to_array(),
                    animated.previous_body_yaw + yaw_delta * animation_fraction,
                    animated.scale,
                ),
                texture_layer: layer as u32,
                route: ActorRigRoute::Compiled,
                tint: 0,
                overlay_rgba8: animated.overlay,
                uv_anim: render::IDENTITY_UV_ANIM,
                light: render::pack_actor_light(0, 15),
            };
            if let Some(equipment) = &fighter.equipment {
                for (slot, item) in equipment.armour.iter().enumerate() {
                    let Some(item) = item else {
                        continue;
                    };
                    let Some(binding) = self.equipment.binding(&item.name) else {
                        continue;
                    };
                    let Some(armor) = self.armor.get(binding.geometry.identifier.as_ref()) else {
                        continue;
                    };
                    let Some(location) = self
                        .textures
                        .get(binding.texture.identifier.as_ref())
                        .copied()
                    else {
                        continue;
                    };
                    let map = view_presentation::armor_pose::bone_map(&armor.names, &rig.names);
                    let mut armor_body = body.clone();
                    armor_body.input.identity.layer = equipment_display::LAYER_HELMET + slot as u8;
                    armor_body.input.rig = armor.id;
                    armor_body.input.previous_bones =
                        view_presentation::armor_pose::remap_pose(&map, &body.input.previous_bones)
                            .into();
                    armor_body.input.current_bones =
                        view_presentation::armor_pose::remap_pose(&map, &body.input.current_bones)
                            .into();
                    armor_body.texture_layer = location.layer();
                    armor_body.tint = if binding.material.contains("leather") {
                        view_presentation::armor_pose::pack_tint(
                            item.color
                                .as_deref()
                                .and_then(parse_rgb)
                                .unwrap_or(assets::DEFAULT_LEATHER_RGB),
                        )
                    } else {
                        0
                    };
                    assignments.insert(armor_body.input.identity, location);
                    submissions.push(armor_body);
                }
            }
            if let (Some(cape), Some(_pixels)) = (&self.cape_rig, self.capes.get(&fighter.id)) {
                let offset = keys[..layer]
                    .iter()
                    .filter(|key| self.capes.contains_key(*key))
                    .count();
                let mut submission = body.clone();
                submission.input.identity.layer = view_presentation::cape::ACTOR_LAYER_CAPE;
                submission.input.rig = cape.id;
                let lerp = |[from, to]: [f32; 2]| from + (to - from) * animation_fraction;
                let [from, to] = animated.java.cape.map(Vec3::from_array);
                let cape_input = render_model::java_animation::JavaCapeInput {
                    chase: from.lerp(to, animation_fraction),
                    body_yaw: animated.java.body_yaw_at(animation_fraction),
                    bob: lerp(animated.java.bob),
                    walked: lerp(animated.java.walked),
                    sneaking: fighter.sneaking,
                };
                let pose = view_presentation::cape::java_cape_pose(
                    cape,
                    &rig.names,
                    |index| animated.rest.get(index).copied(),
                    &body.input.current_bones,
                    &cape_input,
                );
                submission.input.previous_bones = Arc::clone(&pose);
                submission.input.current_bones = pose;
                submission.texture_layer = (keys.len() + offset) as u32;
                submission.tint = 0;
                submission.overlay_rgba8 = 0;
                submissions.push(submission);
            }
            let hand_bones = ["rightItem", "leftItem"].map(|name| {
                rig.names
                    .iter()
                    .position(|bone| bone.eq_ignore_ascii_case(name))
            });
            if let Some(equipment) = &fighter.equipment {
                for (hand, item) in [&equipment.main_hand, &equipment.off_hand]
                    .into_iter()
                    .enumerate()
                {
                    if let Some(item) = item
                        && let Some((layer, location)) = self.held_layer(
                            &body,
                            item,
                            hand_bones[hand],
                            equipment_display::LAYER_MAIN_HAND + hand as u8,
                        )
                    {
                        assignments.insert(layer.input.identity, location);
                        submissions.push(layer);
                    }
                }
            }
            persona_selections.push((body.clone(), self.animator.skin_layers(&fighter.id)));
            submissions.push(body);
        }
        for entity in &current.entities {
            let Some(rig) = self.entities.get(&entity.kind) else {
                continue;
            };
            let Some(location) = self.artwork.route(rig.id) else {
                continue;
            };
            let Some(animated) = self.animator.pose(&entity.id, rig.id, &rig.names) else {
                continue;
            };
            let old = previous
                .and_then(|frame| {
                    frame
                        .entities
                        .iter()
                        .find(|old| old.id == entity.id && old.kind == entity.kind)
                })
                .unwrap_or(entity);
            let position =
                Vec3::from_array(old.position).lerp(Vec3::from_array(entity.position), fraction);
            let identity = ActorRenderIdentity {
                session_id: hash_id(&current.id),
                dimension: 0,
                runtime_id: hash_id(&entity.id),
                spawn_revision: 1,
                ingress_sequence: self.generation,
                source_tick: Some(self.generation),
                movement_revision: self.generation,
                pose_generation: animated.completed_tick,
                layer: render::ACTOR_LAYER_BODY,
            };
            let yaw = self
                .animator
                .actor_world_yaw(&entity.id)
                .unwrap_or(animated.body_yaw);
            assignments.insert(identity, location);
            submissions.push(ActorRigSubmission {
                material: render::ActorMaterial::default(),
                culling_bounds: assets::SkinGeometryBounds::default(),
                input: ActorRigRenderInput {
                    identity,
                    rig: rig.id,
                    previous_bones: animated.previous,
                    current_bones: animated.current,
                    completed_tick: animated.completed_tick,
                    reset_generation: animated.reset_generation,
                },
                world_from_actor: equipment_display::rig_world_from_actor(
                    position.to_array(),
                    yaw,
                    animated.scale,
                ),
                texture_layer: location.layer(),
                route: ActorRigRoute::Compiled,
                tint: 0,
                overlay_rgba8: animated.overlay,
                uv_anim: render::IDENTITY_UV_ANIM,
                light: render::pack_actor_light(0, 15),
            });
        }
        self.persona.apply(
            &mut self.scene,
            &self.artwork,
            &persona_selections,
            &mut submissions,
            &mut assignments,
        );
        self.scene
            .update_rigs_with_artwork(
                animation_fraction,
                None,
                submissions,
                &self.render_skins,
                &assignments,
            )
            .clone()
    }
}
