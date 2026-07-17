use std::{collections::BTreeMap, sync::Arc};

use assets::{EntityAssetKind, ItemActionPhase, RuntimeEntityAssets};
use protocol::{ActorActionEvent, ActorActionKind};

use crate::{ActorLifetimeId, EntityRigId};

pub const MAX_ACTIONS_PER_ACTOR: usize = 32;
pub const MAX_ACTION_EVENTS_PER_TICK: usize = 4_096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActorSourceTick {
    Packet(i64),
    IngressSequence(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ActorEventIdentity {
    pub session_id: u64,
    pub dimension: i32,
    pub actor_lifetime: u64,
    pub ingress_sequence: u64,
    pub source_tick: ActorSourceTick,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteActionFallback {
    None,
    StaticPose,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RemoteActionStats {
    pub static_fallbacks: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RemoteActionSnapshot {
    pub actor: ActorLifetimeId,
    pub event: ActorEventIdentity,
    pub kind: ActorActionKind,
    pub data: f32,
    pub swing_source: Option<Arc<str>>,
    pub phase: ItemActionPhase,
    pub fallback: RemoteActionFallback,
}

#[derive(Debug)]
pub(crate) struct RemoteActionStore {
    assets: Option<Arc<RuntimeEntityAssets>>,
    timelines: BTreeMap<ActorLifetimeId, Vec<RemoteActionSnapshot>>,
    accepted_this_tick: usize,
    stats: RemoteActionStats,
}

impl RemoteActionStore {
    pub(crate) fn diagnostic() -> Self {
        Self::new(None)
    }

    pub(crate) fn with_assets(assets: Arc<RuntimeEntityAssets>) -> Self {
        Self::new(Some(assets))
    }

    fn new(assets: Option<Arc<RuntimeEntityAssets>>) -> Self {
        Self {
            assets,
            timelines: BTreeMap::new(),
            accepted_this_tick: 0,
            stats: RemoteActionStats::default(),
        }
    }

    pub(crate) fn clear(&mut self) {
        self.timelines.clear();
        self.accepted_this_tick = 0;
    }

    pub(crate) fn remove(&mut self, lifetime: ActorLifetimeId) {
        self.timelines.remove(&lifetime);
    }

    pub(crate) fn apply(
        &mut self,
        lifetime: ActorLifetimeId,
        rig: Option<EntityRigId>,
        sequence: u64,
        source_tick: ActorSourceTick,
        event: &ActorActionEvent,
    ) -> bool {
        if !event.data.is_finite()
            || matches!(event.kind, ActorActionKind::Ignored { .. })
            || self.accepted_this_tick >= MAX_ACTION_EVENTS_PER_TICK
        {
            return false;
        }
        let identity = ActorEventIdentity {
            session_id: lifetime.session_id,
            dimension: lifetime.dimension,
            actor_lifetime: lifetime.spawn_revision,
            ingress_sequence: sequence,
            source_tick,
        };
        let fallback = self.fallback(&event.kind, rig);
        let history = self.timelines.entry(lifetime).or_default();
        if history
            .iter()
            .any(|snapshot| snapshot.event == identity && snapshot.kind == event.kind)
        {
            return false;
        }
        if history.len() == MAX_ACTIONS_PER_ACTOR {
            history.remove(0);
        }
        history.push(RemoteActionSnapshot {
            actor: lifetime,
            event: identity,
            kind: event.kind.clone(),
            data: event.data,
            swing_source: event.swing_source.clone(),
            phase: initial_phase(event),
            fallback,
        });
        if fallback == RemoteActionFallback::StaticPose {
            self.stats.static_fallbacks = self.stats.static_fallbacks.saturating_add(1);
        }
        self.accepted_this_tick += 1;
        true
    }

    pub(crate) fn can_accept(&self, count: usize) -> bool {
        self.accepted_this_tick
            .checked_add(count)
            .is_some_and(|total| total <= MAX_ACTION_EVENTS_PER_TICK)
    }

    pub(crate) fn reset_on_teleport(&mut self, lifetime: ActorLifetimeId) {
        if let Some(history) = self.timelines.get_mut(&lifetime)
            && let Some(mut current) = history.pop()
        {
            current.phase = ItemActionPhase::Cancelled;
            history.clear();
            history.push(current);
        }
    }

    pub(crate) fn advance_tick(&mut self) {
        self.accepted_this_tick = 0;
        for current in self
            .timelines
            .values_mut()
            .filter_map(|history| history.last_mut())
        {
            current.phase = match current.phase {
                ItemActionPhase::Idle | ItemActionPhase::Cancelled => current.phase,
                ItemActionPhase::Windup { .. } => ItemActionPhase::Active { elapsed_ticks: 0 },
                ItemActionPhase::Active { .. } => ItemActionPhase::Recover { elapsed_ticks: 0 },
                ItemActionPhase::Recover { .. } => ItemActionPhase::Idle,
                ItemActionPhase::UseHeld {
                    elapsed_ticks,
                    duration_ticks,
                } if elapsed_ticks.saturating_add(1) >= duration_ticks => {
                    ItemActionPhase::Recover { elapsed_ticks: 0 }
                }
                ItemActionPhase::UseHeld {
                    elapsed_ticks,
                    duration_ticks,
                } => ItemActionPhase::UseHeld {
                    elapsed_ticks: elapsed_ticks.saturating_add(1),
                    duration_ticks,
                },
            };
        }
    }

    pub(crate) fn get(&self, lifetime: ActorLifetimeId) -> Option<&RemoteActionSnapshot> {
        self.timelines.get(&lifetime)?.last()
    }

    pub(crate) fn history(&self, lifetime: ActorLifetimeId) -> &[RemoteActionSnapshot] {
        self.timelines.get(&lifetime).map_or(&[], Vec::as_slice)
    }

    pub(crate) const fn stats(&self) -> RemoteActionStats {
        self.stats
    }

    fn fallback(&self, kind: &ActorActionKind, rig: Option<EntityRigId>) -> RemoteActionFallback {
        let ActorActionKind::Custom {
            animation,
            controller,
        } = kind
        else {
            return RemoteActionFallback::None;
        };
        let available = self.assets.as_ref().is_some_and(|assets| {
            let Some(geometry) = rig.and_then(|rig| assets.rig_geometries().get(rig.0 as usize))
            else {
                return false;
            };
            let animation_first = geometry.first_animation as usize;
            let animation_end = animation_first.saturating_add(geometry.animation_count as usize);
            let animation_available = assets
                .rig_animations()
                .get(animation_first..animation_end)
                .is_some_and(|bindings| {
                    bindings.iter().any(|binding| {
                        assets
                            .animation_clips()
                            .get(binding.clip as usize)
                            .and_then(|clip| assets.symbols().get(clip.symbol as usize))
                            .is_some_and(|symbol| {
                                symbol.kind == EntityAssetKind::Animation
                                    && symbol.identifier.as_ref() == animation.as_ref()
                            })
                    })
                });
            let controller_first = geometry.first_controller as usize;
            let controller_end =
                controller_first.saturating_add(geometry.controller_count as usize);
            let controller_available = controller.is_empty()
                || assets
                    .rig_controllers()
                    .get(controller_first..controller_end)
                    .is_some_and(|bindings| {
                        bindings.iter().any(|binding| {
                            assets
                                .controllers()
                                .get(binding.controller as usize)
                                .and_then(|compiled| assets.symbols().get(compiled.symbol as usize))
                                .is_some_and(|symbol| {
                                    symbol.kind == EntityAssetKind::AnimationController
                                        && symbol.identifier.as_ref() == controller.as_ref()
                                })
                        })
                    });
            animation_available && controller_available
        });
        if available {
            RemoteActionFallback::None
        } else {
            RemoteActionFallback::StaticPose
        }
    }
}

fn initial_phase(event: &ActorActionEvent) -> ItemActionPhase {
    match event.kind {
        ActorActionKind::SwingArm
        | ActorActionKind::CriticalHit
        | ActorActionKind::MagicCriticalHit => ItemActionPhase::Windup { elapsed_ticks: 0 },
        ActorActionKind::RowRight | ActorActionKind::RowLeft => ItemActionPhase::UseHeld {
            elapsed_ticks: 0,
            duration_ticks: duration_ticks(event.data),
        },
        ActorActionKind::Wake | ActorActionKind::Custom { .. } => {
            ItemActionPhase::Active { elapsed_ticks: 0 }
        }
        ActorActionKind::Ignored { .. } => ItemActionPhase::Idle,
    }
}

fn duration_ticks(seconds: f32) -> u16 {
    if seconds <= 0.0 {
        return 1;
    }
    (seconds * 20.0).ceil().clamp(1.0, f32::from(u16::MAX)) as u16
}
