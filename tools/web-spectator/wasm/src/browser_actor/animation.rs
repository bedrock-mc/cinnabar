//! Stream observations feed the same compiled fixed-tick Molang animator as native actors.
use std::{collections::HashMap, sync::Arc};

use actor_animation::{
    ACTOR_SWING_TICKS, ACTOR_TICK_DURATION, ActorAnimationStore, ActorRigSnapshot,
    ActorTickContext, AnimationActor, WornArmor,
};
use assets::{RuntimeEntityAssets, RuntimeEquipmentCatalog};
use bevy::platform::time::Instant;
use render::{EntityRigId, RenderBoneTransform};
use render_data::{ActorKind, ActorMetadataValue, ActorStatus, HandPhase, SkinGeometrySource};

use super::super::browser_model::{Fighter, Frame};
use super::{hash_id, parse_rgb};

mod pose_cache;
use pose_cache::PoseCache;
type ConvertedPose = (Arc<[RenderBoneTransform]>, Arc<[RenderBoneTransform]>);

pub(super) struct NativeAnimator {
    store: ActorAnimationStore,
    actors: HashMap<u64, Observation>,
    session: u64,
    last_tick: Instant,
    pending_millis: f64,
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
    fighter: Fighter,
    kind: ActorKind,
    runtime_id: u64,
    position: [f32; 3],
    previous_position: [f32; 3],
    velocity: [f32; 3],
    metadata: HashMap<u32, ActorMetadataValue>,
    int_properties: HashMap<u32, i32>,
    float_properties: HashMap<u32, f32>,
    status: ActorStatus,
    skin: Option<Arc<SkinGeometrySource>>,
    swing: Option<String>,
    hurt: Option<String>,
}

impl NativeAnimator {
    pub(super) fn new(entities: Arc<RuntimeEntityAssets>) -> Self {
        Self {
            store: ActorAnimationStore::with_assets(entities),
            actors: HashMap::new(),
            session: 0,
            last_tick: Instant::now(),
            pending_millis: ACTOR_TICK_DURATION.as_secs_f64() * 1000.0,
            poses: PoseCache::default(),
        }
    }

    pub(super) fn advance(
        &mut self,
        frame: &Frame,
        previous: Option<&Frame>,
        fraction: f32,
        view: AnimationView<'_>,
        equipment: &RuntimeEquipmentCatalog,
        skin: impl Fn(&str) -> Option<Arc<SkinGeometrySource>>,
    ) {
        let AnimationView {
            hidden_player,
            rotation: view_rotation,
            position: view_position,
        } = view;
        let now = js_sys::Date::now();
        let clock = Instant::now();
        let tick_millis = ACTOR_TICK_DURATION.as_secs_f64() * 1000.0;
        let session = hash_id(&format!("{}:{}", frame.id, frame.replay_epoch));
        if self.session != session {
            self.store.clear();
            self.actors.clear();
            self.poses.clear();
            self.session = session;
            self.last_tick = clock;
            self.pending_millis = tick_millis;
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
                    && !frame
                        .entities
                        .iter()
                        .any(|entity| hash_id(&entity.id) == *id)
            })
            .collect::<Vec<_>>();
        for id in removed {
            self.actors.remove(&id);
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
        let entities = frame
            .entities
            .iter()
            .map(|entity| entity.observation())
            .collect::<Vec<_>>();
        let old_entities = previous
            .map(|frame| {
                frame
                    .entities
                    .iter()
                    .map(|entity| entity.observation())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for fighter in frame.fighters.iter().chain(&entities) {
            let runtime_id = hash_id(&fighter.id);
            let old = previous
                .and_then(|old| {
                    old.fighters
                        .iter()
                        .chain(&old_entities)
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
            let actor = self
                .actors
                .entry(runtime_id)
                .or_insert_with(|| Observation {
                    fighter: fighter.clone(),
                    kind: ActorKind::Player {
                        uuid: [0; 16],
                        username: fighter.name.as_str().into(),
                    },
                    runtime_id,
                    position,
                    previous_position: position,
                    velocity,
                    metadata: HashMap::new(),
                    int_properties: HashMap::new(),
                    float_properties: HashMap::new(),
                    status: ActorStatus::default(),
                    skin: None,
                    swing: None,
                    hurt: None,
                });
            actor.fighter.clone_from(fighter);
            actor.kind = frame
                .entities
                .iter()
                .find(|entity| entity.id == fighter.id)
                .map_or_else(
                    || ActorKind::Player {
                        uuid: [0; 16],
                        username: fighter.name.as_str().into(),
                    },
                    |entity| ActorKind::Entity {
                        identifier: entity.kind.as_str().into(),
                    },
                );
            actor.position = position;
            actor.velocity = velocity;
            actor.skin = skin(&fighter.id);
            let flags = [
                (render_data::ACTOR_FLAG_SNEAKING, fighter.sneaking),
                (render_data::ACTOR_FLAG_SPRINTING, fighter.sprinting),
                (render_data::ACTOR_FLAG_USING_ITEM, fighter.using_item),
                (render_data::ACTOR_FLAG_SWIMMING, fighter.swimming),
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
            let swing_identity = fighter.swing_id.as_ref().or(fighter.swing_at.as_ref());
            if actor.swing.as_ref() != swing_identity {
                actor.swing = swing_identity.cloned();
                if recent(
                    fighter.swing_at.as_deref(),
                    now,
                    ACTOR_SWING_TICKS as f64 * tick_millis,
                ) {
                    self.store.start_swing(runtime_id, ACTOR_SWING_TICKS);
                }
            }
            let hurt_identity = fighter.hurt_id.as_ref().or(fighter.hurt_at.as_ref());
            if actor.hurt.as_ref() != hurt_identity {
                actor.hurt = hurt_identity.cloned();
                if recent(
                    fighter.hurt_at.as_deref(),
                    now,
                    f64::from(render_data::HURT_DURATION_TICKS) * tick_millis,
                ) {
                    actor.status.hurt_time = render_data::HURT_DURATION_TICKS;
                    actor.status.skip_red_flash = false;
                }
            }
            if fresh {
                self.pending_millis = self.pending_millis.max(tick_millis);
            }
        }
        let speed = if frame.replay_playing == Some(false) {
            0.0
        } else {
            frame.replay_speed.unwrap_or(1.0).clamp(0.25, 2.0) as f64
        };
        self.pending_millis += clock.duration_since(self.last_tick).as_secs_f64() * 1000.0 * speed;
        self.last_tick = clock;
        if self.pending_millis > tick_millis * 5.0 {
            self.pending_millis = tick_millis * 5.0;
            for runtime_id in self.actors.keys() {
                self.store.mark_reset(*runtime_id);
            }
        }
        let steps = (self.pending_millis / tick_millis).floor() as u32;
        self.pending_millis -= f64::from(steps) * tick_millis;
        for step in 0..steps {
            self.store.advance_tick(
                &self.actors,
                None,
                hidden_player.map(hash_id),
                step + 1 == steps,
                step == 0,
                |actor| {
                    let fighter = &actor.fighter;
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
                    ActorTickContext {
                        animation_elapsed_ticks: Some(steps),
                        main_hand: main.map(|item| item.name.as_str().into()),
                        off_hand: off.map(|item| item.name.as_str().into()),
                        main_hand_max_use_ticks: main_use,
                        is_local_first_person: hidden_player == Some(fighter.id.as_str()),
                        camera_rotation: if matches!(actor.kind, ActorKind::Entity { .. }) {
                            view_rotation
                        } else {
                            [fighter.pitch, fighter.yaw]
                        },
                        camera_position: if matches!(actor.kind, ActorKind::Entity { .. }) {
                            view_position
                        } else {
                            camera_position
                        },
                        armor: std::array::from_fn(|slot| {
                            equipment_items
                                .and_then(|items| items.armour.get(slot))
                                .and_then(Option::as_ref)
                                .map(|item| WornArmor {
                                    item: item.name.as_str().into(),
                                    dye_rgb: item.color.as_deref().and_then(parse_rgb),
                                })
                        }),
                        skin_geometry: actor.skin.clone(),
                        ..ActorTickContext::default()
                    }
                },
            );
            for actor in self.actors.values_mut() {
                actor.previous_position = actor.position;
                actor.status.tick();
            }
        }
    }

    pub(super) fn skin_layers(&self, id: &str) -> Vec<actor_animation::SkinRenderLayer> {
        self.store
            .get(hash_id(id))
            .map(|rig| rig.skin_layers.to_vec())
            .unwrap_or_default()
    }

    pub(super) fn partial_tick(&self) -> f32 {
        (self.pending_millis / (ACTOR_TICK_DURATION.as_secs_f64() * 1000.0)).clamp(0.0, 1.0) as f32
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
            render::pack_overlay_rgba8(render::HURT_OVERLAY_RGBA)
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
    bones: &[actor_animation::BoneTransform],
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
        render_data::item_use::pack_identifier(identifier)
            .and_then(|alias| equipment.item_use_ticks(alias))
    });
    match render_data::item_use::classify(identifier, false, 0, pack)? {
        render_data::item_use::AirUse::Hold { max_ticks, .. } => Some(max_ticks),
        _ => None,
    }
}
fn recent(stamp: Option<&str>, now: f64, duration: f64) -> bool {
    stamp
        .map(js_sys::Date::parse)
        .is_some_and(|stamp| stamp.is_finite() && now - stamp >= -1000.0 && now - stamp <= duration)
}

impl AnimationActor for Observation {
    fn runtime_id(&self) -> u64 {
        self.runtime_id
    }
    fn spawn_revision(&self) -> u64 {
        1
    }
    fn kind(&self) -> &ActorKind {
        &self.kind
    }
    fn position(&self) -> [f32; 3] {
        self.position
    }
    fn previous_position(&self) -> [f32; 3] {
        self.previous_position
    }
    fn velocity(&self) -> [f32; 3] {
        self.velocity
    }
    fn pitch(&self) -> f32 {
        self.fighter.pitch
    }
    fn yaw(&self) -> f32 {
        self.fighter.yaw
    }
    fn head_yaw(&self) -> f32 {
        self.fighter.yaw
    }
    fn body_yaw(&self) -> f32 {
        self.fighter.yaw
    }
    fn on_ground(&self) -> Option<bool> {
        Some(self.fighter.on_ground)
    }
    fn metadata(&self) -> &HashMap<u32, ActorMetadataValue> {
        &self.metadata
    }
    fn int_properties(&self) -> &HashMap<u32, i32> {
        &self.int_properties
    }
    fn float_properties(&self) -> &HashMap<u32, f32> {
        &self.float_properties
    }
    fn status(&self) -> &ActorStatus {
        &self.status
    }
    fn health(&self) -> Option<f32> {
        Some(self.fighter.health)
    }
    fn max_health(&self) -> Option<f32> {
        Some(self.fighter.max_health)
    }
}

pub(super) struct AnimationView<'a> {
    pub hidden_player: Option<&'a str>,
    pub rotation: [f32; 2],
    pub position: [f32; 3],
}
