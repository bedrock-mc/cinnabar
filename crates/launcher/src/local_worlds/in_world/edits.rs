//! Reconciles each setting's saves with edits made while the worker is busy.

use super::{Action, View, apply, update};
use crate::local_worlds::{
    model::Effect,
    options::{OptionChanges, WorldOption},
    world::World,
};
use bridge::GameMode;
use std::collections::BTreeMap;

/// A setting's confirmed value, pending saves and latest unresolved save failure.
#[derive(Debug)]
struct Setting<T> {
    confirmed: T,
    saved_revision: u64,
    pending: BTreeMap<u64, T>,
    failure: Option<(u64, String)>,
}

impl<T: Copy> Setting<T> {
    /// Starts tracking a setting at its last confirmed value.
    fn new(confirmed: T) -> Self {
        Self {
            confirmed,
            saved_revision: 0,
            pending: BTreeMap::new(),
            failure: None,
        }
    }

    /// Refreshes the confirmed value on success and returns the newest pending or confirmed value.
    fn finish(&mut self, revision: u64, confirmed_revision: u64, result: Result<T, &str>) -> T {
        let pending = self.pending.remove(&revision).is_some();
        if let Ok(saved) = result
            && revision == confirmed_revision
        {
            self.confirmed = saved;
        }
        if pending && revision > self.saved_revision {
            match result {
                Ok(_) => {
                    self.saved_revision = revision;
                    self.failure.take_if(|(failed, _)| *failed <= revision);
                }
                Err(error)
                    if self
                        .failure
                        .as_ref()
                        .is_none_or(|(failed, _)| *failed < revision) =>
                {
                    self.failure = Some((revision, error.to_owned()));
                }
                Err(_) => {}
            }
        }
        self.pending
            .last_key_value()
            .filter(|(revision, _)| **revision > confirmed_revision)
            .map_or(self.confirmed, |(_, value)| *value)
    }
}

/// The settings edited during one visit to a world.
#[derive(Debug, Default)]
struct Changes {
    confirmed_revision: u64,
    game_mode: Option<Setting<GameMode>>,
    difficulty: Option<Setting<super::Difficulty>>,
    options: BTreeMap<WorldOption, Setting<i32>>,
}

impl Changes {
    /// Whether this visit still has an edit awaiting the given reply.
    fn pending(&self, revision: u64) -> bool {
        self.game_mode
            .as_ref()
            .is_some_and(|setting| setting.pending.contains_key(&revision))
            || self
                .difficulty
                .as_ref()
                .is_some_and(|setting| setting.pending.contains_key(&revision))
            || self
                .options
                .values()
                .any(|setting| setting.pending.contains_key(&revision))
    }

    /// Returns the newest unresolved failure across this world's edited settings.
    fn error(&self) -> Option<&str> {
        self.game_mode
            .iter()
            .filter_map(|setting| setting.failure.as_ref())
            .chain(
                self.difficulty
                    .iter()
                    .filter_map(|setting| setting.failure.as_ref()),
            )
            .chain(
                self.options
                    .values()
                    .filter_map(|setting| setting.failure.as_ref()),
            )
            .max_by_key(|(revision, _)| *revision)
            .map(|(_, error)| error.as_str())
    }
}

/// Save revisions stay unique across visits so a late reply cannot acknowledge a new edit.
#[derive(Debug, Default)]
pub(crate) struct Edits {
    revision: u64,
    worlds: BTreeMap<String, Changes>,
}

impl Edits {
    /// Forgets the previous visit's confirmed values, pending edits and error message.
    pub(crate) fn reset(&mut self) {
        self.worlds.clear();
    }

    /// Shows a valid action immediately and tags every affected setting with its save revision.
    pub(crate) fn change(
        &mut self,
        worlds: &mut [World],
        id: &str,
        action: Action,
    ) -> Option<Effect> {
        let world = worlds.iter_mut().find(|world| world.id == id)?;
        let update = update(world, action)?;
        self.revision += 1;
        let revision = self.revision;
        let changes = self.worlds.entry(id.to_owned()).or_default();
        if let Some(value) = update.game_mode {
            changes
                .game_mode
                .get_or_insert_with(|| Setting::new(world.game_mode))
                .pending
                .insert(revision, value);
        }
        if let Some(value) = update.difficulty {
            changes
                .difficulty
                .get_or_insert_with(|| Setting::new(world.difficulty))
                .pending
                .insert(revision, value);
        }
        if let Some(options) = &update.options {
            for &id in WorldOption::ALL {
                if let Some(value) = options.get(id) {
                    changes
                        .options
                        .entry(id)
                        .or_insert_with(|| Setting::new(world.options.get(id)))
                        .pending
                        .insert(revision, value);
                }
            }
        }
        apply(world, &update);
        Some(Effect::Apply {
            id: id.to_owned(),
            revision,
            update,
        })
    }

    /// Refreshes confirmed settings from successful replies, retaining only newer pending choices.
    pub(crate) fn finish(
        &mut self,
        worlds: &mut [World],
        id: &str,
        revision: u64,
        result: Result<&World, String>,
    ) {
        let Some(changes) = self.worlds.get_mut(id) else {
            return;
        };
        if !changes.pending(revision) {
            return;
        }
        let Some(world) = worlds.iter_mut().find(|world| world.id == id) else {
            return;
        };
        let result = result.as_deref().map_err(String::as_str);
        if let Ok(saved) = result
            && revision > changes.confirmed_revision
        {
            world.game_mode = saved.game_mode;
            world.difficulty = saved.difficulty;
            world.options.clone_from(&saved.options);
            changes.confirmed_revision = revision;
        }
        if let Some(setting) = &mut changes.game_mode {
            world.game_mode = setting.finish(
                revision,
                changes.confirmed_revision,
                result.map(|world| world.game_mode),
            );
        }
        if let Some(setting) = &mut changes.difficulty {
            world.difficulty = setting.finish(
                revision,
                changes.confirmed_revision,
                result.map(|world| world.difficulty),
            );
        }
        let mut options = OptionChanges::default();
        for (&id, setting) in &mut changes.options {
            options.insert(
                id,
                setting.finish(
                    revision,
                    changes.confirmed_revision,
                    result.map(|world| world.options.get(id)),
                ),
            );
        }
        world.options.apply_changes(&options);
    }

    /// Builds the Game section with any failure from this world's latest edits.
    pub(crate) fn view(&self, world: &World) -> Option<View> {
        let mut view = View::of(world)?;
        view.error = self
            .worlds
            .get(&world.id)
            .and_then(Changes::error)
            .map(str::to_owned);
        Some(view)
    }
}
