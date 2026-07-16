use std::collections::HashMap;

use protocol::{
    ActorAttribute, ActorEvent, ActorKind, ActorMetadataValue, ActorMoveEvent, ActorProperty,
    ActorSpawnEvent, MAX_ACTOR_ATTRIBUTES, MAX_ACTOR_METADATA_ENTRIES, MAX_ACTOR_PROPERTIES,
    MovePlayerEvent, PlayerListEntry, PlayerSkin,
};

pub(crate) const MAX_TRACKED_ACTORS: usize = 8_192;
pub(crate) const MAX_TRACKED_PLAYERS: usize = 4_096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActorApplyResult {
    Inserted,
    Replaced,
    Updated,
    Removed,
    Reset,
    MissingActor,
    CapacityRejected,
    StaleSession,
    StaleSequence,
    StaleDimension,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ActorSnapshot {
    pub unique_id: i64,
    pub runtime_id: u64,
    pub kind: ActorKind,
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub pitch: f32,
    pub yaw: f32,
    pub head_yaw: f32,
    pub body_yaw: f32,
    pub on_ground: Option<bool>,
    pub teleported: bool,
    pub metadata: HashMap<i32, ActorMetadataValue>,
    pub attributes: HashMap<std::sync::Arc<str>, ActorAttribute>,
    pub int_properties: HashMap<i32, i32>,
    pub float_properties: HashMap<i32, f32>,
}

impl From<ActorSpawnEvent> for ActorSnapshot {
    fn from(spawn: ActorSpawnEvent) -> Self {
        let mut snapshot = Self {
            unique_id: spawn.unique_id,
            runtime_id: spawn.runtime_id,
            kind: spawn.kind,
            position: spawn.position,
            velocity: spawn.velocity,
            pitch: spawn.pitch,
            yaw: spawn.yaw,
            head_yaw: spawn.head_yaw,
            body_yaw: spawn.body_yaw,
            on_ground: None,
            teleported: false,
            metadata: HashMap::with_capacity(spawn.metadata.len()),
            attributes: HashMap::with_capacity(spawn.attributes.len()),
            int_properties: HashMap::new(),
            float_properties: HashMap::new(),
        };
        snapshot.apply_metadata(&spawn.metadata);
        snapshot.apply_attributes(&spawn.attributes);
        snapshot.apply_properties(&spawn.properties);
        snapshot
    }
}

impl ActorSnapshot {
    fn apply_metadata(&mut self, metadata: &[protocol::ActorMetadata]) -> bool {
        let mut rejected = false;
        for metadata in metadata {
            if self.metadata.len() >= MAX_ACTOR_METADATA_ENTRIES
                && !self.metadata.contains_key(&metadata.key)
            {
                rejected = true;
                continue;
            }
            self.metadata.insert(metadata.key, metadata.value.clone());
        }
        rejected
    }

    fn apply_attributes(&mut self, attributes: &[ActorAttribute]) -> bool {
        let mut rejected = false;
        for attribute in attributes {
            if self.attributes.len() >= MAX_ACTOR_ATTRIBUTES
                && !self.attributes.contains_key(&attribute.name)
            {
                rejected = true;
                continue;
            }
            self.attributes
                .insert(attribute.name.clone(), attribute.clone());
        }
        rejected
    }

    fn apply_properties(&mut self, properties: &[ActorProperty]) -> bool {
        let mut rejected = false;
        for property in properties {
            match *property {
                ActorProperty::Int { index, value } => {
                    if !self.int_properties.contains_key(&index)
                        && !self.float_properties.contains_key(&index)
                        && self.int_properties.len() + self.float_properties.len()
                            >= MAX_ACTOR_PROPERTIES
                    {
                        rejected = true;
                        continue;
                    }
                    self.float_properties.remove(&index);
                    self.int_properties.insert(index, value);
                }
                ActorProperty::Float { index, value } => {
                    if !self.float_properties.contains_key(&index)
                        && !self.int_properties.contains_key(&index)
                        && self.int_properties.len() + self.float_properties.len()
                            >= MAX_ACTOR_PROPERTIES
                    {
                        rejected = true;
                        continue;
                    }
                    self.int_properties.remove(&index);
                    self.float_properties.insert(index, value);
                }
            }
        }
        rejected
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlayerProfile {
    pub unique_id: i64,
    pub username: std::sync::Arc<str>,
    pub verified: bool,
    pub skin: PlayerSkin,
}

/// Sparse, session-scoped actor state. It owns no render or chunk-mesh state.
#[derive(Debug)]
pub(crate) struct ActorStore {
    session_id: u64,
    dimension: i32,
    latest_sequence: u64,
    max_actors: usize,
    max_players: usize,
    actors: HashMap<u64, ActorSnapshot>,
    unique_to_runtime: HashMap<i64, u64>,
    players: HashMap<[u8; 16], PlayerProfile>,
}

impl ActorStore {
    #[must_use]
    pub(crate) fn new(session_id: u64, dimension: i32) -> Self {
        Self::with_capacity(
            session_id,
            dimension,
            MAX_TRACKED_ACTORS,
            MAX_TRACKED_PLAYERS,
        )
    }

    #[must_use]
    pub(crate) fn with_capacity(
        session_id: u64,
        dimension: i32,
        max_actors: usize,
        max_players: usize,
    ) -> Self {
        Self {
            session_id,
            dimension,
            latest_sequence: 0,
            max_actors,
            max_players,
            actors: HashMap::new(),
            unique_to_runtime: HashMap::new(),
            players: HashMap::new(),
        }
    }

    #[cfg(test)]
    pub(crate) fn begin_session(&mut self, session_id: u64, dimension: i32) {
        self.session_id = session_id;
        self.dimension = dimension;
        self.latest_sequence = 0;
        self.actors.clear();
        self.unique_to_runtime.clear();
        self.players.clear();
    }

    pub(crate) fn reset_dimension(
        &mut self,
        session_id: u64,
        sequence: u64,
        dimension: i32,
    ) -> ActorApplyResult {
        let guard = self.guard(session_id, sequence);
        if guard != ActorApplyResult::Updated {
            return guard;
        }
        self.dimension = dimension;
        self.actors.clear();
        self.unique_to_runtime.clear();
        ActorApplyResult::Reset
    }

    pub(crate) fn apply(
        &mut self,
        session_id: u64,
        sequence: u64,
        event: ActorEvent,
    ) -> ActorApplyResult {
        let guard = self.guard(session_id, sequence);
        if guard != ActorApplyResult::Updated {
            return guard;
        }
        if event_dimension(&event).is_some_and(|dimension| dimension != self.dimension) {
            return ActorApplyResult::StaleDimension;
        }
        match event {
            ActorEvent::Spawn(spawn) => self.apply_spawn(spawn),
            ActorEvent::Remove(remove) => self.remove_unique(remove.unique_id),
            ActorEvent::Move(movement) => {
                let Some(actor) = self.actors.get_mut(&movement.runtime_id) else {
                    return ActorApplyResult::MissingActor;
                };
                for (target, source) in actor.position.iter_mut().zip(movement.position) {
                    if let Some(source) = source {
                        *target = source;
                    }
                }
                if let Some(value) = movement.pitch {
                    actor.pitch = value;
                }
                if let Some(value) = movement.yaw {
                    actor.yaw = value;
                }
                if let Some(value) = movement.head_yaw {
                    actor.head_yaw = value;
                }
                if let Some(value) = movement.on_ground {
                    actor.on_ground = Some(value);
                }
                actor.teleported = movement.teleported;
                ActorApplyResult::Updated
            }
            ActorEvent::Metadata(update) => {
                let Some(actor) = self.actors.get_mut(&update.runtime_id) else {
                    return ActorApplyResult::MissingActor;
                };
                let rejected = actor.apply_metadata(&update.metadata)
                    | actor.apply_properties(&update.properties);
                if rejected {
                    ActorApplyResult::CapacityRejected
                } else {
                    ActorApplyResult::Updated
                }
            }
            ActorEvent::Attributes(update) => {
                let Some(actor) = self.actors.get_mut(&update.runtime_id) else {
                    return ActorApplyResult::MissingActor;
                };
                if actor.apply_attributes(&update.attributes) {
                    ActorApplyResult::CapacityRejected
                } else {
                    ActorApplyResult::Updated
                }
            }
            ActorEvent::PlayerList(update) => {
                let mut capacity_rejected = false;
                for entry in update.entries.iter() {
                    match entry {
                        PlayerListEntry::Add {
                            uuid,
                            unique_id,
                            username,
                            verified,
                            skin,
                        } => {
                            if self.players.len() >= self.max_players
                                && !self.players.contains_key(uuid)
                            {
                                capacity_rejected = true;
                                continue;
                            }
                            self.players.insert(
                                *uuid,
                                PlayerProfile {
                                    unique_id: *unique_id,
                                    username: username.clone(),
                                    verified: *verified,
                                    skin: skin.clone(),
                                },
                            );
                        }
                        PlayerListEntry::Remove { uuid } => {
                            self.players.remove(uuid);
                        }
                    }
                }
                if capacity_rejected {
                    ActorApplyResult::CapacityRejected
                } else {
                    ActorApplyResult::Updated
                }
            }
        }
    }

    pub(crate) fn apply_player_move(
        &mut self,
        session_id: u64,
        sequence: u64,
        dimension: i32,
        movement: MovePlayerEvent,
    ) -> ActorApplyResult {
        self.apply(
            session_id,
            sequence,
            ActorEvent::Move(ActorMoveEvent {
                dimension,
                runtime_id: movement.runtime_id,
                position: movement.position.map(Some),
                pitch: Some(movement.pitch),
                yaw: Some(movement.yaw),
                head_yaw: Some(movement.yaw),
                on_ground: None,
                teleported: false,
            }),
        )
    }

    fn guard(&mut self, session_id: u64, sequence: u64) -> ActorApplyResult {
        if session_id != self.session_id {
            return ActorApplyResult::StaleSession;
        }
        if sequence <= self.latest_sequence {
            return ActorApplyResult::StaleSequence;
        }
        self.latest_sequence = sequence;
        ActorApplyResult::Updated
    }

    fn apply_spawn(&mut self, spawn: ActorSpawnEvent) -> ActorApplyResult {
        let replaces_runtime = self.actors.contains_key(&spawn.runtime_id);
        let replaces_unique = self.unique_to_runtime.contains_key(&spawn.unique_id);
        if self.actors.len() >= self.max_actors && !replaces_runtime && !replaces_unique {
            return ActorApplyResult::CapacityRejected;
        }

        let mut replaced = false;
        if let Some(previous) = self.actors.remove(&spawn.runtime_id) {
            self.unique_to_runtime.remove(&previous.unique_id);
            replaced = true;
        }
        if let Some(previous_runtime) = self.unique_to_runtime.remove(&spawn.unique_id) {
            self.actors.remove(&previous_runtime);
            replaced = true;
        }
        let runtime_id = spawn.runtime_id;
        let unique_id = spawn.unique_id;
        self.actors.insert(runtime_id, spawn.into());
        self.unique_to_runtime.insert(unique_id, runtime_id);
        if replaced {
            ActorApplyResult::Replaced
        } else {
            ActorApplyResult::Inserted
        }
    }

    fn remove_unique(&mut self, unique_id: i64) -> ActorApplyResult {
        let Some(runtime_id) = self.unique_to_runtime.remove(&unique_id) else {
            return ActorApplyResult::MissingActor;
        };
        self.actors.remove(&runtime_id);
        ActorApplyResult::Removed
    }

    #[must_use]
    pub(crate) fn render_players(&self) -> Vec<(&ActorSnapshot, Option<&PlayerProfile>)> {
        let mut players = self
            .actors
            .values()
            .filter_map(|actor| {
                let ActorKind::Player { uuid, .. } = &actor.kind else {
                    return None;
                };
                let profile = self
                    .players
                    .get(uuid)
                    .filter(|profile| profile.unique_id == actor.unique_id);
                Some((actor, profile))
            })
            .collect::<Vec<_>>();
        players.sort_unstable_by_key(|(actor, _)| actor.runtime_id);
        players
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) fn get(&self, runtime_id: u64) -> Option<&ActorSnapshot> {
        self.actors.get(&runtime_id)
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) fn len(&self) -> usize {
        self.actors.len()
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.actors.is_empty()
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) fn player_count(&self) -> usize {
        self.players.len()
    }
}

fn event_dimension(event: &ActorEvent) -> Option<i32> {
    match event {
        ActorEvent::Spawn(event) => Some(event.dimension),
        ActorEvent::Remove(event) => Some(event.dimension),
        ActorEvent::Move(event) => Some(event.dimension),
        ActorEvent::Metadata(event) => Some(event.dimension),
        ActorEvent::Attributes(event) => Some(event.dimension),
        ActorEvent::PlayerList(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use protocol::{
        ActorAttribute, ActorAttributesUpdateEvent, ActorEvent, ActorKind, ActorMetadata,
        ActorMetadataUpdateEvent, ActorMetadataValue, ActorMoveEvent, ActorProperty,
        ActorRemoveEvent, ActorSpawnEvent, PlayerListEntry, PlayerListUpdateEvent,
    };

    use super::{ActorApplyResult, ActorStore};

    fn spawn(runtime_id: u64, unique_id: i64) -> ActorEvent {
        ActorEvent::Spawn(ActorSpawnEvent {
            dimension: 0,
            unique_id,
            runtime_id,
            kind: ActorKind::Entity {
                identifier: "minecraft:bee".into(),
            },
            position: [1.0, 2.0, 3.0],
            velocity: [0.0; 3],
            pitch: 0.0,
            yaw: 0.0,
            head_yaw: 0.0,
            body_yaw: 0.0,
            metadata: Arc::from([]),
            attributes: Arc::from([]),
            properties: Arc::from([]),
        })
    }

    #[test]
    fn actor_lifecycle_applies_fifo_patches_and_removes_by_unique_id() {
        let mut store = ActorStore::new(11, 0);
        assert_eq!(
            store.apply(11, 1, spawn(42, -7)),
            ActorApplyResult::Inserted
        );
        assert_eq!(
            store.apply(
                11,
                2,
                ActorEvent::Move(ActorMoveEvent {
                    dimension: 0,
                    runtime_id: 42,
                    position: [Some(9.0), None, Some(8.0)],
                    pitch: Some(10.0),
                    yaw: None,
                    head_yaw: None,
                    on_ground: Some(true),
                    teleported: false,
                }),
            ),
            ActorApplyResult::Updated
        );
        assert_eq!(
            store.apply(
                11,
                3,
                ActorEvent::Metadata(ActorMetadataUpdateEvent {
                    dimension: 0,
                    runtime_id: 42,
                    metadata: Arc::from([ActorMetadata {
                        key: 4,
                        value: ActorMetadataValue::String("Beeatrice".into()),
                    }]),
                    properties: Arc::from([ActorProperty::Int { index: 2, value: 5 }]),
                    tick: 10,
                }),
            ),
            ActorApplyResult::Updated
        );
        assert_eq!(
            store.apply(
                11,
                4,
                ActorEvent::Attributes(ActorAttributesUpdateEvent {
                    dimension: 0,
                    runtime_id: 42,
                    attributes: Arc::from([ActorAttribute {
                        name: "minecraft:health".into(),
                        min: 0.0,
                        max: 20.0,
                        current: 17.0,
                        default: Some(20.0),
                        modifiers: Arc::from([]),
                    }]),
                    tick: 11,
                }),
            ),
            ActorApplyResult::Updated
        );

        let actor = store.get(42).expect("stored actor");
        assert_eq!(actor.position, [9.0, 2.0, 8.0]);
        assert_eq!(actor.pitch, 10.0);
        assert_eq!(actor.on_ground, Some(true));
        assert_eq!(
            actor.metadata[&4],
            ActorMetadataValue::String("Beeatrice".into())
        );
        assert_eq!(actor.attributes["minecraft:health"].current, 17.0);

        assert_eq!(
            store.apply(
                11,
                5,
                ActorEvent::Remove(ActorRemoveEvent {
                    dimension: 0,
                    unique_id: -7,
                }),
            ),
            ActorApplyResult::Removed
        );
        assert!(store.get(42).is_none());
    }

    #[test]
    fn duplicate_runtime_or_unique_ids_replace_atomically() {
        let mut store = ActorStore::new(1, 0);
        store.apply(1, 1, spawn(10, 20));
        assert_eq!(store.apply(1, 2, spawn(10, 21)), ActorApplyResult::Replaced);
        assert_eq!(store.len(), 1);
        assert_eq!(store.get(10).unwrap().unique_id, 21);

        assert_eq!(store.apply(1, 3, spawn(11, 21)), ActorApplyResult::Replaced);
        assert_eq!(store.len(), 1);
        assert!(store.get(10).is_none());
        assert_eq!(store.get(11).unwrap().unique_id, 21);
    }

    #[test]
    fn capacity_is_bounded_but_existing_actor_replacement_is_allowed() {
        let mut store = ActorStore::with_capacity(1, 0, 2, 2);
        assert_eq!(store.apply(1, 1, spawn(1, 1)), ActorApplyResult::Inserted);
        assert_eq!(store.apply(1, 2, spawn(2, 2)), ActorApplyResult::Inserted);
        assert_eq!(
            store.apply(1, 3, spawn(3, 3)),
            ActorApplyResult::CapacityRejected
        );
        assert_eq!(store.apply(1, 4, spawn(2, 22)), ActorApplyResult::Replaced);
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn cumulative_actor_patches_cannot_exceed_retained_collection_bounds() {
        use protocol::{MAX_ACTOR_ATTRIBUTES, MAX_ACTOR_METADATA_ENTRIES, MAX_ACTOR_PROPERTIES};

        let mut store = ActorStore::new(1, 0);
        store.apply(1, 1, spawn(1, 1));

        let metadata = (0..MAX_ACTOR_METADATA_ENTRIES)
            .map(|key| ActorMetadata {
                key: i32::try_from(key).unwrap(),
                value: ActorMetadataValue::Int(i32::try_from(key).unwrap()),
            })
            .collect::<Vec<_>>();
        assert_eq!(
            store.apply(
                1,
                2,
                ActorEvent::Metadata(ActorMetadataUpdateEvent {
                    dimension: 0,
                    runtime_id: 1,
                    metadata: metadata.into(),
                    properties: Arc::from([]),
                    tick: 1,
                }),
            ),
            ActorApplyResult::Updated
        );
        assert_eq!(
            store.apply(
                1,
                3,
                ActorEvent::Metadata(ActorMetadataUpdateEvent {
                    dimension: 0,
                    runtime_id: 1,
                    metadata: Arc::from([ActorMetadata {
                        key: i32::try_from(MAX_ACTOR_METADATA_ENTRIES).unwrap(),
                        value: ActorMetadataValue::Int(1),
                    }]),
                    properties: Arc::from([]),
                    tick: 2,
                }),
            ),
            ActorApplyResult::CapacityRejected
        );
        assert_eq!(
            store.get(1).unwrap().metadata.len(),
            MAX_ACTOR_METADATA_ENTRIES
        );

        let attributes = (0..MAX_ACTOR_ATTRIBUTES)
            .map(|index| ActorAttribute {
                name: format!("attribute.{index}").into(),
                min: 0.0,
                max: 1.0,
                current: 1.0,
                default: None,
                modifiers: Arc::from([]),
            })
            .collect::<Vec<_>>();
        store.apply(
            1,
            4,
            ActorEvent::Attributes(ActorAttributesUpdateEvent {
                dimension: 0,
                runtime_id: 1,
                attributes: attributes.into(),
                tick: 3,
            }),
        );
        assert_eq!(
            store.apply(
                1,
                5,
                ActorEvent::Attributes(ActorAttributesUpdateEvent {
                    dimension: 0,
                    runtime_id: 1,
                    attributes: Arc::from([ActorAttribute {
                        name: "attribute.overflow".into(),
                        min: 0.0,
                        max: 1.0,
                        current: 1.0,
                        default: None,
                        modifiers: Arc::from([]),
                    }]),
                    tick: 4,
                }),
            ),
            ActorApplyResult::CapacityRejected
        );
        assert_eq!(store.get(1).unwrap().attributes.len(), MAX_ACTOR_ATTRIBUTES);

        let properties = (0..MAX_ACTOR_PROPERTIES)
            .map(|index| ActorProperty::Int {
                index: i32::try_from(index).unwrap(),
                value: 1,
            })
            .collect::<Vec<_>>();
        store.apply(
            1,
            6,
            ActorEvent::Metadata(ActorMetadataUpdateEvent {
                dimension: 0,
                runtime_id: 1,
                metadata: Arc::from([]),
                properties: properties.into(),
                tick: 5,
            }),
        );
        assert_eq!(
            store.apply(
                1,
                7,
                ActorEvent::Metadata(ActorMetadataUpdateEvent {
                    dimension: 0,
                    runtime_id: 1,
                    metadata: Arc::from([]),
                    properties: Arc::from([ActorProperty::Float {
                        index: i32::try_from(MAX_ACTOR_PROPERTIES).unwrap(),
                        value: 1.0,
                    }]),
                    tick: 6,
                }),
            ),
            ActorApplyResult::CapacityRejected
        );
        let actor = store.get(1).unwrap();
        assert_eq!(
            actor.int_properties.len() + actor.float_properties.len(),
            MAX_ACTOR_PROPERTIES
        );
    }

    #[test]
    fn stale_session_sequence_and_dimension_are_rejected() {
        let mut store = ActorStore::new(5, 0);
        assert_eq!(
            store.apply(4, 1, spawn(1, 1)),
            ActorApplyResult::StaleSession
        );
        assert_eq!(store.apply(5, 2, spawn(1, 1)), ActorApplyResult::Inserted);
        assert_eq!(
            store.apply(5, 2, spawn(2, 2)),
            ActorApplyResult::StaleSequence
        );
        let mut wrong_dimension = spawn(3, 3);
        let ActorEvent::Spawn(spawn) = &mut wrong_dimension else {
            unreachable!()
        };
        spawn.dimension = 1;
        assert_eq!(
            store.apply(5, 3, wrong_dimension),
            ActorApplyResult::StaleDimension
        );
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn dimension_reset_clears_actors_and_session_reset_also_clears_roster() {
        let mut store = ActorStore::new(1, 0);
        store.apply(1, 1, spawn(1, 1));
        store.apply(
            1,
            2,
            ActorEvent::PlayerList(PlayerListUpdateEvent {
                entries: Arc::from([PlayerListEntry::Add {
                    uuid: [7; 16],
                    unique_id: 1,
                    username: "Alex".into(),
                    verified: true,
                    skin: protocol::PlayerSkin::Unavailable(
                        protocol::PlayerSkinUnavailable::InvalidDimensions,
                    ),
                }]),
            }),
        );
        assert_eq!(store.player_count(), 1);

        assert_eq!(store.reset_dimension(1, 3, 2), ActorApplyResult::Reset);
        assert!(store.is_empty());
        assert_eq!(store.player_count(), 1);

        store.begin_session(2, 0);
        assert!(store.is_empty());
        assert_eq!(store.player_count(), 0);
        assert_eq!(
            store.apply(1, 4, spawn(2, 2)),
            ActorApplyResult::StaleSession
        );
    }

    #[test]
    fn render_players_join_roster_skins_and_sort_by_runtime_id() {
        let skin = protocol::PlayerSkin::Standard(protocol::StandardSkin {
            width: 64,
            height: 64,
            rgba8: vec![9; 64 * 64 * 4].into(),
        });
        let mut store = ActorStore::new(1, 0);
        for (sequence, runtime_id, unique_id, uuid) in [(1, 20, 2, [2; 16]), (2, 10, 1, [1; 16])] {
            let mut event = spawn(runtime_id, unique_id);
            let ActorEvent::Spawn(spawn) = &mut event else {
                unreachable!()
            };
            spawn.kind = ActorKind::Player {
                uuid,
                username: format!("player-{runtime_id}").into(),
            };
            store.apply(1, sequence, event);
        }
        store.apply(
            1,
            3,
            ActorEvent::PlayerList(PlayerListUpdateEvent {
                entries: Arc::from([PlayerListEntry::Add {
                    uuid: [1; 16],
                    unique_id: 1,
                    username: "player-10".into(),
                    verified: true,
                    skin: skin.clone(),
                }]),
            }),
        );

        let players = store.render_players();
        assert_eq!(
            players
                .iter()
                .map(|(actor, _)| actor.runtime_id)
                .collect::<Vec<_>>(),
            [10, 20]
        );
        assert_eq!(players[0].1.map(|profile| &profile.skin), Some(&skin));
        assert!(players[1].1.is_none());
    }
}
