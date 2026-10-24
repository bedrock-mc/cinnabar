//! Entity snapshot reads and transaction staging for the current world opening.
use super::*;
use crate::{
    limits::{MAX_ENTITIES, MAX_STAGED_OPS},
    protocol as p,
};

impl From<p::EntityHandle> for wit::EntityHandle {
    /// Converts a checked actor identity to the guest record.
    fn from(v: p::EntityHandle) -> Self {
        Self {
            id: v.id,
            generation: v.generation,
        }
    }
}
impl From<wit::EntityHandle> for p::EntityHandle {
    /// Converts an opaque guest identity without granting authority.
    fn from(v: wit::EntityHandle) -> Self {
        Self {
            id: v.id,
            generation: v.generation,
        }
    }
}
impl From<p::EntityRotation> for wit::EntityRotation {
    /// Preserves the actor's yaw and pitch in degrees.
    fn from(v: p::EntityRotation) -> Self {
        Self {
            yaw: v.yaw,
            pitch: v.pitch,
        }
    }
}
impl From<wit::EntityRotation> for p::EntityRotation {
    /// Preserves the actor's yaw and pitch for validation.
    fn from(v: wit::EntityRotation) -> Self {
        Self {
            yaw: v.yaw,
            pitch: v.pitch,
        }
    }
}
impl From<p::EntityState> for wit::EntityState {
    /// Converts an owned snapshot, including its ordered declared properties.
    fn from(v: p::EntityState) -> Self {
        Self {
            handle: v.handle.into(),
            kind: wit::EntityTypeId { value: v.kind },
            position: v.position.into(),
            rotation: v.rotation.into(),
            velocity: v.velocity.into(),
            name_tag: v.name_tag,
            scale: v.scale,
            variant: v.variant,
            properties: v
                .properties
                .into_iter()
                .map(|p| wit::EntityProperty {
                    id: wit::EntityPropertyId { value: p.id },
                    value: p.value,
                })
                .collect(),
        }
    }
}
impl From<wit::EntityState> for p::EntityState {
    /// Converts a staged state while retaining its identity for authority checks.
    fn from(v: wit::EntityState) -> Self {
        Self {
            handle: v.handle.into(),
            kind: v.kind.value,
            position: v.position.into(),
            rotation: v.rotation.into(),
            velocity: v.velocity.into(),
            name_tag: v.name_tag,
            scale: v.scale,
            variant: v.variant,
            properties: v
                .properties
                .into_iter()
                .map(|p| p::EntityProperty {
                    id: p.id.value,
                    value: p.value,
                })
                .collect(),
        }
    }
}
impl From<p::EntityRider> for wit::EntityRider {
    /// Converts a transaction-owned rider observation.
    fn from(v: p::EntityRider) -> Self {
        Self {
            player: v.player,
            seat: v.seat,
        }
    }
}

/// Checks interaction item data before exposing an entity observation to the guest.
pub(super) fn event(v: p::EntityEvent) -> Result<wit::EntityEvent, String> {
    let interaction = |player, hand, item: Option<p::ItemStack>| -> Result<_, String> {
        let item = item
            .map(|s| -> Result<_, String> {
                Ok(wit::ItemStack {
                    id: s.id,
                    metadata: s.metadata,
                    count: s.count,
                    max_count: s.max_count,
                    data: decode(s.data.as_deref())?,
                    plain: s.plain,
                })
            })
            .transpose()?;
        Ok(wit::EntityInteraction {
            player,
            hand: match hand {
                p::EntityHand::Main => wit::EntityHand::Main,
                p::EntityHand::Off => wit::EntityHand::Off,
            },
            item,
        })
    };
    Ok(match v {
        p::EntityEvent::Interact { player, hand, item } => {
            wit::EntityEvent::Interact(interaction(player, hand, item)?)
        }
        p::EntityEvent::Attacked { player, hand, item } => {
            wit::EntityEvent::Attacked(interaction(player, hand, item)?)
        }
        p::EntityEvent::Mounted { rider } => wit::EntityEvent::Mounted(rider.into()),
        p::EntityEvent::Dismounted { rider } => wit::EntityEvent::Dismounted(rider.into()),
        p::EntityEvent::Steering { input } => wit::EntityEvent::Steering(wit::EntityInput {
            rider: input.rider.into(),
            sideways: input.sideways,
            forward: input.forward,
            yaw: input.yaw,
            pitch: input.pitch,
            jump: input.jump,
            sneak: input.sneak,
        }),
        p::EntityEvent::Despawned {} => wit::EntityEvent::Despawned,
    })
}

impl CallbackRes {
    /// Reads owned actors in this callback's instance, with staged writes applied.
    pub(crate) fn entities(&mut self) -> Result<Vec<wit::EntityState>> {
        self.host_call()?;
        Ok(self
            .context
            .as_ref()
            .map(|c| c.entities.iter().map(|s| s.state.clone().into()).collect())
            .unwrap_or_default())
    }
    /// Reserves a fresh handle from this event's sequence and stages its initial state.
    pub(crate) fn spawn_entity(
        &mut self,
        kind: wit::EntityTypeId,
        position: wit::Position,
        rotation: wit::EntityRotation,
    ) -> Result<Result<wit::EntityHandle, WorldError>> {
        self.host_call()?;
        let Some(context) = &mut self.context else {
            return Ok(Err(WorldError::Unavailable));
        };
        let Some(def) = self.own.entities.iter().find(|d| d.id == kind.value) else {
            return Ok(Err(WorldError::NotOwned));
        };
        if context.entities.len() >= MAX_ENTITIES {
            return Ok(Err(WorldError::QuotaExceeded));
        }
        let Some(id) = self
            .info
            .event_sequence
            .checked_mul(MAX_STAGED_OPS as u64)
            .and_then(|base| base.checked_add(self.ops.len() as u64 + 1))
        else {
            return Ok(Err(WorldError::QuotaExceeded));
        };
        let handle = p::EntityHandle {
            id,
            generation: context.current.generation,
        };
        let state = p::EntityState {
            handle,
            kind: kind.value,
            position: position.into(),
            rotation: rotation.into(),
            velocity: p::Position {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            name_tag: String::new(),
            scale: 1.0,
            variant: 0,
            properties: def
                .properties
                .iter()
                .map(|p| p::EntityProperty {
                    id: p.id.clone(),
                    value: p.initial,
                })
                .collect(),
        };
        if !crate::load::entities::valid_state(&state, def) {
            return Ok(Err(WorldError::OutOfBounds));
        }
        stage(
            &mut self.ops,
            Op::Entity {
                action: p::EntityOp::Spawn {
                    state: state.clone(),
                },
            },
        )?;
        self.context
            .as_mut()
            .expect("checked context")
            .entities
            .push(p::EntitySnapshot {
                state,
                revision: 0,
                riders: Vec::new(),
            });
        Ok(Ok(handle.into()))
    }
    /// Updates only an actor whose current generation is present in the callback snapshot.
    pub(crate) fn update_entity(
        &mut self,
        state: wit::EntityState,
        teleport: bool,
    ) -> Result<Result<(), WorldError>> {
        self.host_call()?;
        let state: p::EntityState = state.into();
        let Some(context) = &mut self.context else {
            return Ok(Err(WorldError::Unavailable));
        };
        let Some(index) = context
            .entities
            .iter()
            .position(|s| s.state.handle == state.handle && s.state.kind == state.kind)
        else {
            return Ok(Err(WorldError::Unavailable));
        };
        let Some(def) = self.own.entities.iter().find(|d| d.id == state.kind) else {
            return Ok(Err(WorldError::NotOwned));
        };
        if !crate::load::entities::valid_state(&state, def) {
            return Ok(Err(WorldError::OutOfBounds));
        }
        stage(
            &mut self.ops,
            Op::Entity {
                action: p::EntityOp::Update {
                    state: state.clone(),
                    teleport,
                },
            },
        )?;
        context.entities[index].state = state;
        Ok(Ok(()))
    }
    /// Removes the actor from the staged view as soon as its despawn is admitted.
    pub(crate) fn despawn_entity(
        &mut self,
        handle: wit::EntityHandle,
    ) -> Result<Result<(), WorldError>> {
        self.host_call()?;
        let handle = handle.into();
        let Some(context) = &mut self.context else {
            return Ok(Err(WorldError::Unavailable));
        };
        let Some(index) = context
            .entities
            .iter()
            .position(|s| s.state.handle == handle)
        else {
            return Ok(Err(WorldError::Unavailable));
        };
        stage(
            &mut self.ops,
            Op::Entity {
                action: p::EntityOp::Despawn { handle },
            },
        )?;
        context.entities.remove(index);
        Ok(Ok(()))
    }
    /// Stages boarding for the actor after checking the visible seat definition and occupancy.
    pub(crate) fn mount_entity(
        &mut self,
        handle: wit::EntityHandle,
        seat: u32,
    ) -> Result<Result<(), WorldError>> {
        self.host_call()?;
        let handle = handle.into();
        let Some(actor) = &self.actor else {
            return Ok(Err(WorldError::PlayerUnavailable));
        };
        let Some(context) = &self.context else {
            return Ok(Err(WorldError::Unavailable));
        };
        let Some(entity) = context.entities.iter().find(|s| s.state.handle == handle) else {
            return Ok(Err(WorldError::Unavailable));
        };
        let Some(def) = self.own.entities.iter().find(|d| d.id == entity.state.kind) else {
            return Ok(Err(WorldError::NotOwned));
        };
        if seat as usize >= def.seats.len()
            || entity.riders.iter().any(|r| r.seat == seat)
            || context
                .entities
                .iter()
                .any(|s| s.riders.iter().any(|r| &r.player == actor))
        {
            return Ok(Err(WorldError::Unavailable));
        }
        stage(
            &mut self.ops,
            Op::Entity {
                action: p::EntityOp::Mount { handle, seat },
            },
        )?;
        self.context
            .as_mut()
            .expect("checked context")
            .entities
            .iter_mut()
            .find(|s| s.state.handle == handle)
            .expect("checked entity")
            .riders
            .push(p::EntityRider {
                player: actor.clone(),
                seat,
            });
        Ok(Ok(()))
    }
    /// Requests dismount for the live actor; the adapter checks its current vehicle.
    pub(crate) fn dismount_entity(&mut self) -> Result<Result<(), WorldError>> {
        self.host_call()?;
        let Some(actor) = &self.actor else {
            return Ok(Err(WorldError::PlayerUnavailable));
        };
        let Some(context) = &mut self.context else {
            return Ok(Err(WorldError::Unavailable));
        };
        let Some(entity) = context
            .entities
            .iter_mut()
            .find(|s| s.riders.iter().any(|r| &r.player == actor))
        else {
            return Ok(Err(WorldError::Unavailable));
        };
        stage(
            &mut self.ops,
            Op::Entity {
                action: p::EntityOp::Dismount {},
            },
        )?;
        entity.riders.retain(|r| &r.player != actor);
        Ok(Ok(()))
    }
}

/// Checks the adapter's entity scope before the guest receives any handles.
pub(super) fn check_context(own: &Catalog, context: &p::WorldContext) -> Result<(), String> {
    if context.entities.len() > MAX_ENTITIES {
        return Err("entity snapshot exceeds extension policy".into());
    }
    let mut handles = HashSet::new();
    let mut riders = HashSet::new();
    for entity in &context.entities {
        let Some(def) = own.entities.iter().find(|d| d.id == entity.state.kind) else {
            return Err("entity snapshot names a foreign type".into());
        };
        if entity.state.handle.generation != context.current.generation
            || entity.state.handle.id == 0
            || entity.revision == 0
            || !handles.insert(entity.state.handle)
            || !crate::load::entities::valid_state(&entity.state, def)
        {
            return Err("invalid entity snapshot".into());
        }
        let mut seats = HashSet::new();
        for rider in &entity.riders {
            player_id("rider", Some(&rider.player))?;
            if rider.seat as usize >= def.seats.len()
                || !seats.insert(rider.seat)
                || !riders.insert(&rider.player)
            {
                return Err("invalid entity seat occupancy".into());
            }
        }
    }
    Ok(())
}
