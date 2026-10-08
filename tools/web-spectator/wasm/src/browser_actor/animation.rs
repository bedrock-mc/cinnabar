//! Stream observations feed the same compiled fixed-tick Molang animator as native actors.
use std::{collections::HashMap, sync::Arc};

use assets::{RuntimeEntityAssets, RuntimeEquipmentCatalog};
use bevy::platform::time::Instant;
use client_world::{
    ACTOR_SWING_TICKS, ACTOR_TICK_DURATION, ActorAnimationStore, ActorRigSnapshot,
    ActorTickContext, WornArmor,
};
use client_world::{ActorPose, ActorSnapshot, HandPhase};
use protocol::{
    ActorAttribute, ActorKind, ActorMetadataValue, ActorSpawnEvent, NetworkItemStack,
    SkinGeometrySource,
};
use render_model::{EntityRigId, RenderBoneTransform};

use super::super::browser_model::{Fighter, Frame};
use super::{hash_id, parse_rgb};

mod pose_cache;
use pose_cache::PoseCache;
type ConvertedPose = (Arc<[RenderBoneTransform]>, Arc<[RenderBoneTransform]>);

pub(super) struct NativeAnimator {
    store: ActorAnimationStore,
    actors: HashMap<u64, ActorSnapshot>,
    observations: HashMap<u64, Observation>,
    session: u64,
    last_tick: Instant,
    poses: PoseCache<ConvertedPose>,
}

pub(super) struct AnimatedPose {
    pub previous: Arc<[RenderBoneTransform]>,
    pub current: Arc<[RenderBoneTransform]>,
    pub hand: [HandPhase; 2],
    pub previous_body_yaw: f32,
    pub body_yaw: f32,
    pub completed_tick: u64,
    pub reset_generation: u64,
    pub scale: f32,
    pub overlay: u32,
}

struct Observation {
    skin: Option<Arc<SkinGeometrySource>>,
    swing: Option<String>,
    hurt: Option<String>,
}

impl NativeAnimator {
    pub(super) fn new(entities: Arc<RuntimeEntityAssets>) -> Self {
        Self {
            store: ActorAnimationStore::with_assets(entities),
            actors: HashMap::new(),
            observations: HashMap::new(),
            session: 0,
            last_tick: Instant::now() - ACTOR_TICK_DURATION,
            poses: PoseCache::default(),
        }
    }

    pub(super) fn advance(
        &mut self,
        frame: &Frame,
        previous: Option<&Frame>,
        fraction: f32,
        hidden_player: Option<&str>,
        equipment: &RuntimeEquipmentCatalog,
        skin: impl Fn(&str) -> Option<Arc<SkinGeometrySource>>,
    ) {
        let now = js_sys::Date::now();
        let clock = Instant::now();
        let tick_millis = ACTOR_TICK_DURATION.as_secs_f64() * 1000.0;
        let session = hash_id(&frame.id);
        if self.session != session {
            self.store.clear();
            self.actors.clear();
            self.observations.clear();
            self.poses.clear();
            self.session = session;
            self.last_tick = clock - ACTOR_TICK_DURATION;
        }
        let removed = self
            .actors
            .keys()
            .copied()
            .filter(|id| {
                !frame
                    .fighters
                    .iter()
                    .any(|fighter| hash_id(&fighter.id) == *id)
            })
            .collect::<Vec<_>>();
        for id in removed {
            self.actors.remove(&id);
            self.observations.remove(&id);
            self.store.remove_runtime(id);
            self.poses.invalidate(id);
        }
        let interval = previous
            .map(|old| {
                (js_sys::Date::parse(&frame.updated_at) - js_sys::Date::parse(&old.updated_at))
                    / 1000.0
            })
            .filter(|v| v.is_finite() && *v > 0.0)
            .unwrap_or(ACTOR_TICK_DURATION.as_secs_f64())
            .clamp(ACTOR_TICK_DURATION.as_secs_f64(), 1.0) as f32;
        for fighter in &frame.fighters {
            let runtime_id = hash_id(&fighter.id);
            let old = previous
                .and_then(|old| {
                    old.fighters
                        .iter()
                        .find(|old| old.id == fighter.id && old.dead == fighter.dead)
                })
                .unwrap_or(fighter);
            let position = std::array::from_fn(|axis| {
                old.position[axis] + (fighter.position[axis] - old.position[axis]) * fraction
            });
            let velocity = std::array::from_fn(|axis| {
                (fighter.position[axis] - old.position[axis]) / interval
                    * ACTOR_TICK_DURATION.as_secs_f32()
            });
            let fresh = !self.actors.contains_key(&runtime_id);
            let actor = self.actors.entry(runtime_id).or_insert_with(|| {
                ActorSnapshot::from_observation(
                    ActorSpawnEvent {
                        dimension: 0,
                        unique_id: runtime_id as i64,
                        runtime_id,
                        kind: ActorKind::Player {
                            uuid: [0; 16],
                            username: fighter.name.as_str().into(),
                        },
                        position,
                        velocity,
                        pitch: fighter.pitch,
                        yaw: fighter.yaw,
                        head_yaw: fighter.yaw,
                        body_yaw: fighter.yaw,
                        held_item: NetworkItemStack::empty(),
                        metadata: Arc::from([]),
                        attributes: Arc::from([health_attribute(fighter)]),
                        properties: Arc::from([]),
                        links: Arc::from([]),
                    },
                    1,
                )
            });
            let observation = self
                .observations
                .entry(runtime_id)
                .or_insert_with(|| Observation {
                    skin: None,
                    swing: None,
                    hurt: None,
                });
            actor.kind = ActorKind::Player {
                uuid: [0; 16],
                username: fighter.name.as_str().into(),
            };
            actor.position = position;
            actor.observe_velocity(velocity);
            actor.pitch = fighter.pitch;
            actor.yaw = fighter.yaw;
            actor.head_yaw = fighter.yaw;
            actor.body_yaw = fighter.yaw;
            actor.on_ground = Some(fighter.on_ground);
            actor.movement_revision = actor.movement_revision.wrapping_add(1).max(1);
            if let Some(health) = actor.attributes.get_mut("minecraft:health") {
                health.current = fighter.health;
                health.max = fighter.max_health;
            }
            observation.skin = skin(&fighter.id);
            let flags = [
                (client_world::ACTOR_FLAG_SNEAKING, fighter.sneaking),
                (client_world::ACTOR_FLAG_SPRINTING, fighter.sprinting),
                (client_world::ACTOR_FLAG_USING_ITEM, fighter.using_item),
                (client_world::ACTOR_FLAG_SWIMMING, fighter.swimming),
            ]
            .into_iter()
            .fold(0_u64, |flags, (bit, set)| {
                flags | if set { 1_u64 << bit } else { 0 }
            });
            actor.metadata.insert(0, ActorMetadataValue::Flags(flags));
            actor
                .metadata
                .insert(4, ActorMetadataValue::String(fighter.name.as_str().into()));
            actor.status.fluid = fighter.swimming.then_some((true, false));
            if fighter.dead && !actor.status.dead {
                actor.status.die();
            } else if !fighter.dead && actor.status.dead {
                actor.status.revive();
            }
            if fresh {
                self.store.insert(session, 0, actor);
            }
            if observation.swing != fighter.swing_at {
                observation.swing.clone_from(&fighter.swing_at);
                if recent(
                    fighter.swing_at.as_deref(),
                    now,
                    ACTOR_SWING_TICKS as f64 * tick_millis,
                ) {
                    self.store.start_swing(runtime_id, ACTOR_SWING_TICKS);
                }
            }
            if observation.hurt != fighter.hurt_at {
                observation.hurt.clone_from(&fighter.hurt_at);
                if recent(
                    fighter.hurt_at.as_deref(),
                    now,
                    f64::from(client_world::HURT_DURATION_TICKS) * tick_millis,
                ) {
                    actor.status.hurt_time = client_world::HURT_DURATION_TICKS;
                    actor.status.skip_red_flash = false;
                }
            }
            if fresh {
                self.last_tick = self.last_tick.min(clock - ACTOR_TICK_DURATION);
            }
        }
        self.store.begin_skin_preparation();
        for observation in self.observations.values() {
            if let Some(source) = &observation.skin {
                self.store.request_skin_preparation(source);
            }
        }
        self.store.submit_skin_preparation();
        self.store.begin_skin_preparation();
        let elapsed = clock.duration_since(self.last_tick).as_secs_f64() * 1000.0;
        let steps = (elapsed / tick_millis).floor().min(5.0) as u32;
        if elapsed > tick_millis * 5.0 {
            self.last_tick = clock - ACTOR_TICK_DURATION * steps;
            for runtime_id in self.actors.keys() {
                self.store.mark_reset(*runtime_id);
            }
        }
        let observations = &self.observations;
        for step in 0..steps {
            self.last_tick += ACTOR_TICK_DURATION;
            self.store.advance_tick(
                &self.actors,
                None,
                hidden_player.map(hash_id),
                step + 1 == steps,
                step == 0,
                |actor| {
                    let Some(fighter) = frame
                        .fighters
                        .iter()
                        .find(|fighter| hash_id(&fighter.id) == actor.runtime_id)
                    else {
                        return ActorTickContext::default();
                    };
                    let equipment_items = fighter.equipment.as_ref();
                    let main = equipment_items
                        .and_then(|items| items.main_hand.as_ref())
                        .or_else(|| {
                            fighter.pov.as_ref().and_then(|pov| {
                                pov.hotbar.get(pov.selected_slot).and_then(Option::as_ref)
                            })
                        });
                    let off = equipment_items.and_then(|items| items.off_hand.as_ref());
                    let main_use = main
                        .and_then(|item| use_ticks(equipment, &item.name))
                        .unwrap_or(0);
                    let mut camera_position = actor.position;
                    camera_position[1] += fighter.pov.as_ref().map_or(0.0, |pov| pov.eye_height);
                    let mut context = ActorTickContext::default();
                    context.animation_elapsed_ticks = Some(steps);
                    context.main_hand = main.map(|item| item.name.as_str().into());
                    context.off_hand = off.map(|item| item.name.as_str().into());
                    context.main_hand_max_use_ticks = main_use;
                    context.main_hand_metadata = main.map_or(0, |item| item.meta.max(0) as u32);
                    context.main_hand_slot = fighter
                        .pov
                        .as_ref()
                        .map_or(0, |pov| pov.selected_slot as u8);
                    context.is_local_first_person = hidden_player == Some(fighter.id.as_str());
                    context.camera_rotation = [fighter.pitch, fighter.yaw];
                    context.camera_position = camera_position;
                    context.armor = std::array::from_fn(|slot| {
                        equipment_items
                            .and_then(|items| items.armour.get(slot))
                            .and_then(Option::as_ref)
                            .map(|item| WornArmor {
                                item: item.name.as_str().into(),
                                dye_rgb: item.color.as_deref().and_then(parse_rgb),
                            })
                    });
                    context.skin_geometry = observations
                        .get(&actor.runtime_id)
                        .and_then(|observation| observation.skin.clone());
                    context
                },
            );
            for actor in self.actors.values_mut() {
                actor.previous_pose = ActorPose {
                    position: actor.position,
                    pitch: actor.pitch,
                    yaw: actor.yaw,
                    head_yaw: actor.head_yaw,
                };
                actor.status.tick();
            }
        }
    }

    pub(super) fn partial_tick(&self) -> f32 {
        (Instant::now().duration_since(self.last_tick).as_secs_f32()
            / ACTOR_TICK_DURATION.as_secs_f32())
        .clamp(0.0, 1.0)
    }

    pub(super) fn invalidate_pose(&mut self, id: &str) {
        self.poses.invalidate(hash_id(id));
    }

    pub(super) fn pose(
        &mut self,
        id: &str,
        rig: EntityRigId,
        names: &[Box<str>],
    ) -> Option<AnimatedPose> {
        let runtime_id = hash_id(id);
        let pose = self.store.get(runtime_id)?;
        let actor = self.actors.get(&runtime_id)?;
        let (previous, current) = self.poses.get_or_insert_with(
            runtime_id,
            (
                rig.0,
                pose.completed_tick,
                pose.reset_generation,
                pose.rest_completed_tick,
                pose.rest_reset_generation,
            ),
            || {
                Some((
                    convert_pose(&pose, pose.previous, names)?,
                    convert_pose(&pose, pose.current, names)?,
                ))
            },
        )?;
        let overlay = if actor.status.overlay_active() {
            render::pack_overlay_rgba8(view_presentation::equipment_display::hurt_overlay_rgba(
                client_world::HURT_OVERLAY_ALPHA,
            ))
        } else {
            0
        };
        Some(AnimatedPose {
            previous: Arc::clone(previous),
            current: Arc::clone(current),
            hand: pose.hand,
            previous_body_yaw: pose.previous_body_yaw,
            body_yaw: pose.body_yaw,
            completed_tick: pose.completed_tick,
            reset_generation: pose.reset_generation,
            scale: pose.scale,
            overlay,
        })
    }
}

fn convert_pose(
    pose: &ActorRigSnapshot<'_>,
    bones: &[client_world::BoneTransform],
    names: &[Box<str>],
) -> Option<Arc<[RenderBoneTransform]>> {
    names
        .iter()
        .map(|name| {
            let index = pose
                .bone_names
                .iter()
                .position(|posed| posed.eq_ignore_ascii_case(name))?;
            let bone = bones.get(index)?;
            RenderBoneTransform::from_model_space_scaled(
                bone.rotation,
                bone.translation_scale,
                bone.axis_scale,
            )
        })
        .collect::<Option<Vec<_>>>()
        .map(Arc::from)
}

pub(super) fn use_ticks(equipment: &RuntimeEquipmentCatalog, identifier: &str) -> Option<u32> {
    let pack = equipment.item_use_ticks(identifier).or_else(|| {
        gameplay::item_use::classify::pack_identifier(identifier)
            .and_then(|alias| equipment.item_use_ticks(alias))
    });
    match gameplay::item_use::classify(identifier, false, 0, pack)? {
        gameplay::item_use::AirUse::Hold { max_ticks, .. } => Some(max_ticks),
        _ => None,
    }
}
fn recent(stamp: Option<&str>, now: f64, duration: f64) -> bool {
    stamp
        .map(js_sys::Date::parse)
        .is_some_and(|stamp| stamp.is_finite() && now - stamp >= -1000.0 && now - stamp <= duration)
}

fn health_attribute(fighter: &Fighter) -> ActorAttribute {
    ActorAttribute {
        name: "minecraft:health".into(),
        min: 0.0,
        max: fighter.max_health,
        current: fighter.health,
        default: None,
        modifiers: Arc::from([]),
    }
}
