//! Snapshot reads and staged lifecycle/state operations for server WIT 0.7.
use super::*;
use crate::protocol::{
    PlayerEvent, PlayerLocation, Position, StoreKey, StoreScope, WorldHandle, WorldOp,
};

impl From<WorldHandle> for wit::WorldHandle {
    fn from(value: WorldHandle) -> Self {
        Self {
            id: wit::WorldInstanceId { value: value.id },
            generation: value.generation,
        }
    }
}
impl From<wit::WorldHandle> for WorldHandle {
    fn from(value: wit::WorldHandle) -> Self {
        Self {
            id: value.id.value,
            generation: value.generation,
        }
    }
}
impl From<Position> for wit::Position {
    fn from(p: Position) -> Self {
        Self {
            x: p.x,
            y: p.y,
            z: p.z,
        }
    }
}
impl From<wit::Position> for Position {
    fn from(p: wit::Position) -> Self {
        Self {
            x: p.x,
            y: p.y,
            z: p.z,
        }
    }
}
impl From<PlayerLocation> for wit::PlayerLocation {
    fn from(p: PlayerLocation) -> Self {
        Self {
            world: p.world.into(),
            epoch: p.epoch,
            position: p.position.into(),
        }
    }
}
impl From<PlayerEvent> for wit::PlayerEvent {
    fn from(event: PlayerEvent) -> Self {
        match event {
            PlayerEvent::Join { location } => Self::Join(location.into()),
            PlayerEvent::Leave { location } => Self::Leave(location.into()),
            PlayerEvent::WorldChange { before, after } => {
                Self::WorldChange(wit::PlayerTransition {
                    before: before.into(),
                    after: after.into(),
                })
            }
            PlayerEvent::Move { before, after } => Self::Move(wit::PlayerTransition {
                before: before.into(),
                after: after.into(),
            }),
        }
    }
}
impl From<wit::StoreKey> for StoreKey {
    fn from(key: wit::StoreKey) -> Self {
        Self {
            scope: match key.scope {
                wit::StoreScope::Player => StoreScope::Player,
                wit::StoreScope::Instance => StoreScope::Instance,
                wit::StoreScope::Mod => StoreScope::Mod,
            },
            schema: key.schema,
        }
    }
}
impl CallbackRes {
    /// Returns the instance opening in the host snapshot, never a network dimension ID.
    pub(crate) fn world(&mut self) -> Result<wit::WorldHandle> {
        self.host_call()?;
        Ok(self
            .context
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("world context missing"))?
            .current
            .clone()
            .into())
    }
    /// Reads only the callback actor's admitted location.
    pub(crate) fn player_location(
        &mut self,
        player: String,
    ) -> Result<Result<wit::PlayerLocation, WorldError>> {
        self.host_call()?;
        if self.actor.as_ref() != Some(&player) {
            return Ok(Err(WorldError::Denied));
        }
        Ok(self
            .context
            .as_ref()
            .and_then(|c| c.player.clone())
            .map(Into::into)
            .ok_or(WorldError::PlayerUnavailable))
    }
    /// Resolves only instances included in this mod's snapshot.
    pub(crate) fn resolve_world(
        &mut self,
        id: wit::WorldInstanceId,
    ) -> Result<Result<wit::WorldHandle, WorldError>> {
        self.host_call()?;
        Ok(self
            .context
            .as_ref()
            .and_then(|c| c.instances.iter().find(|h| h.id == id.value))
            .cloned()
            .map(Into::into)
            .ok_or(WorldError::Unavailable))
    }
    /// Stages a creation using one definition declared at startup.
    pub(crate) fn create_world(
        &mut self,
        instance: wit::WorldInstance,
    ) -> Result<Result<(), WorldError>> {
        self.host_call()?;
        if !self
            .own
            .worlds
            .definitions
            .iter()
            .any(|d| d.id == instance.definition.value)
        {
            return Ok(Err(WorldError::Denied));
        }
        let owner = instance
            .definition
            .value
            .split(':')
            .next()
            .unwrap_or_default();
        if !crate::load::worlds::owned_name(&instance.id.value, owner) {
            return Ok(Err(WorldError::Denied));
        }
        self.lifecycle(WorldOp::Create {
            instance: protocol::WorldInstance {
                id: instance.id.value,
                definition: instance.definition.value,
            },
        })
    }
    /// Stages unloading a currently authorized instance opening.
    pub(crate) fn unload_world(
        &mut self,
        instance: wit::WorldHandle,
    ) -> Result<Result<(), WorldError>> {
        self.host_call()?;
        let instance: WorldHandle = instance.into();
        if !self
            .context
            .as_ref()
            .is_some_and(|c| c.instances.contains(&instance))
        {
            return Ok(Err(WorldError::Unavailable));
        }
        self.lifecycle(WorldOp::Unload { instance })
    }
    /// Stages one actor transfer; the adapter rechecks ownership and generations before detach.
    pub(crate) fn transfer(
        &mut self,
        player: String,
        instance: wit::WorldHandle,
        position: wit::Position,
    ) -> Result<Result<(), WorldError>> {
        self.host_call()?;
        if self.actor.as_ref() != Some(&player) {
            return Ok(Err(WorldError::Denied));
        }
        let instance: WorldHandle = instance.into();
        if !self
            .context
            .as_ref()
            .is_some_and(|c| c.instances.contains(&instance))
        {
            return Ok(Err(WorldError::Unavailable));
        }
        if ![position.x, position.y, position.z]
            .iter()
            .all(|n| n.is_finite())
        {
            return Ok(Err(WorldError::OutOfBounds));
        }
        self.lifecycle(WorldOp::Transfer {
            player,
            instance,
            position: position.into(),
        })
    }
    /// Enforces a lifecycle-only result so a failed transfer cannot partially commit data.
    fn lifecycle(&mut self, action: WorldOp) -> Result<Result<(), WorldError>> {
        if !self.ops.is_empty() {
            return Ok(Err(WorldError::Denied));
        }
        stage(&mut self.ops, Op::World { action })?;
        Ok(Ok(()))
    }
    /// Reads a schema-matched record. The low-level API lets callers migrate an older revision.
    pub(crate) fn store_read(
        &mut self,
        key: wit::StoreKey,
    ) -> Result<Result<Option<Vec<u8>>, WorldError>> {
        self.host_call()?;
        let key: StoreKey = key.into();
        if key.scope == StoreScope::Player && self.actor.is_none() {
            return Ok(Err(WorldError::Denied));
        }
        let record = self
            .context
            .as_ref()
            .and_then(|c| c.stores.iter().find(|r| r.key.scope == key.scope));
        match record {
            Some(record) if record.data.is_some() && record.key.schema != key.schema => {
                Ok(Err(WorldError::Unavailable))
            }
            Some(record) => Ok(Ok(decode(record.data.as_deref())?)),
            None => Ok(Ok(None)),
        }
    }
    /// Stages one record per scope, retaining absent versus empty and sharing block quota.
    pub(crate) fn store_write(
        &mut self,
        key: wit::StoreKey,
        data: Option<Vec<u8>>,
    ) -> Result<Result<(), WorldError>> {
        self.host_call()?;
        let key: StoreKey = key.into();
        if key.scope == StoreScope::Player && self.actor.is_none() {
            return Ok(Err(WorldError::Denied));
        }
        let Some(context) = self.context.as_mut() else {
            return Ok(Err(WorldError::Unavailable));
        };
        let old = context
            .stores
            .iter()
            .find(|r| r.key.scope == key.scope)
            .and_then(|r| r.data.as_ref())
            .map_or(0, |d| d.len() / 2);
        let size = data.as_ref().map_or(0, Vec::len);
        if size > MAX_BLOCK_DATA_BYTES {
            return Ok(Err(WorldError::TooLarge));
        }
        let earlier = self
            .ops
            .iter()
            .position(|op| matches!(op, Op::StoreWrite { key: old, .. } if old.scope == key.scope));
        let staged = self.staged_data + size - earlier.map_or(0, |i| staged_len(&self.ops[i]));
        if staged > MAX_STAGED_DATA_BYTES {
            return Ok(Err(WorldError::TooLarge));
        }
        let added = self.added + size as i64 - old as i64;
        if added > self.budget as i64 {
            return Ok(Err(WorldError::QuotaExceeded));
        }
        let data = data.map(|bytes| hex::encode(&bytes));
        let op = Op::StoreWrite {
            key,
            data: data.clone(),
        };
        if let Some(index) = self
            .ops
            .iter()
            .position(|op| matches!(op, Op::StoreWrite { key: old, .. } if old.scope == key.scope))
        {
            self.ops[index] = op;
        } else {
            stage(&mut self.ops, op)?;
        }
        if let Some(record) = context.stores.iter_mut().find(|r| r.key.scope == key.scope) {
            record.key = key;
            record.data = data;
        }
        self.staged_data = staged;
        self.added = added;
        Ok(Ok(()))
    }
}
