//! Per-actor Molang variable storage. Slots are plain data, with strings kept in a side table,
//! so copying a variable set is a memory copy rather than a per-value clone.

use std::cell::RefCell;

use super::*;

pub(crate) mod frame;

/// One stored value. A string names its entry in the owning set's string table.
#[derive(Clone, Copy, Debug, Default)]
enum Slot {
    #[default]
    Unset,
    Number(f32),
    String(u32),
    ActorReference(u64),
}

/// A stored value borrowed for comparison; equality matches [`MolangValue`]'s.
#[derive(Debug, PartialEq)]
enum SlotView<'a> {
    Number(f32),
    String(&'a str),
    ActorReference(u64),
}

/// One actor's Molang variables and random stream; an unassigned variable reads as 0.0.
#[derive(Debug, Default)]
pub(crate) struct MolangVariables {
    pub(crate) frame_cache: Option<super::frame_cache::Expressions>,
    rollback: Option<Vec<(Place, Option<MolangValue>)>>,
    values: Vec<Slot>,
    temps: Vec<Slot>,
    /// Temporary slots assigned since the last clear; every assigned temporary is listed.
    live_temps: Vec<usize>,
    /// Strings of `values` and `temps`; each string slot owns exactly one live entry.
    strings: Vec<Option<Arc<str>>>,
    /// Released `strings` entries, reused before the table grows.
    free_strings: Vec<u32>,
    random: u64,
    random_draws: u64,
    publication_random: Option<(u64, u64)>,
    capture_writes: bool,
    writes: Vec<u64>,
}

impl Clone for MolangVariables {
    fn clone(&self) -> Self {
        let Self {
            frame_cache: _,
            rollback: _,
            values,
            temps,
            live_temps,
            strings,
            free_strings,
            random,
            random_draws,
            publication_random,
            capture_writes,
            writes,
        } = self;
        Self {
            frame_cache: None,
            rollback: None,
            values: values.clone(),
            temps: temps.clone(),
            live_temps: live_temps.clone(),
            strings: strings.clone(),
            free_strings: free_strings.clone(),
            random: *random,
            random_draws: *random_draws,
            publication_random: *publication_random,
            capture_writes: *capture_writes,
            writes: writes.clone(),
        }
    }

    /// Copies into this set's existing buffers instead of allocating new ones.
    fn clone_from(&mut self, source: &Self) {
        let Self {
            frame_cache: _,
            rollback: _,
            values,
            temps,
            live_temps,
            strings,
            free_strings,
            random,
            random_draws,
            publication_random,
            capture_writes,
            writes,
        } = source;
        self.frame_cache = None;
        self.rollback = None;
        self.values.clone_from(values);
        self.temps.clone_from(temps);
        self.live_temps.clone_from(live_temps);
        self.strings.clone_from(strings);
        self.free_strings.clone_from(free_strings);
        self.random = *random;
        self.random_draws = *random_draws;
        self.publication_random = *publication_random;
        self.capture_writes = *capture_writes;
        self.writes.clone_from(writes);
    }
}

impl MolangVariables {
    /// Clears scratch values and string ownership while retaining their allocated buffers.
    pub(crate) fn reset(&mut self, seed: u64) {
        #[cfg(test)]
        super::selection_work::record(|work| work.reset += self.values.len() + self.temps.len());
        self.values.fill(Slot::Unset);
        self.temps.fill(Slot::Unset);
        self.live_temps.clear();
        self.strings.clear();
        self.free_strings.clear();
        self.restore_random(seed | 1);
    }

    /// Returns the random state used to validate an observed scratch evaluation.
    pub(super) fn random_state(&self) -> u64 {
        self.random
    }

    /// Restores a scratch random stream before another predicate uses it.
    pub(super) fn restore_random(&mut self, state: u64) {
        self.random = state;
        self.random_draws = 0;
    }

    /// Marks the temporary-list prefix retained before a scratch predicate assigns new slots.
    pub(super) fn temporary_checkpoint(&self) -> usize {
        self.live_temps.len()
    }

    /// Restores journaled slots and drops the temporary-list suffix added by their writes.
    /// Scratch evaluation keeps the original prefix intact until this restoration.
    pub(super) fn restore(
        &mut self,
        journal: impl IntoIterator<Item = (Place, Option<MolangValue>)>,
        temporary_checkpoint: usize,
    ) {
        for (place, value) in journal {
            #[cfg(test)]
            super::selection_work::record(|work| work.restored += 1);
            self.put(place, value);
        }
        self.live_temps.truncate(temporary_checkpoint);
    }

    /// An unassigned set sized for one layout's variables and temporaries.
    pub(super) fn fresh(layout: &VariableLayout, seed: u64) -> Self {
        Self {
            values: vec![Slot::Unset; layout.variable_count],
            temps: vec![Slot::Unset; layout.temp_count],
            random: seed | 1,
            ..Default::default()
        }
    }

    /// The value a slot holds; `None` when unassigned.
    fn load(&self, slot: Slot) -> Option<MolangValue> {
        match slot {
            Slot::Unset => None,
            Slot::Number(value) => Some(MolangValue::Number(value)),
            Slot::String(index) => self.strings[index as usize]
                .clone()
                .map(MolangValue::String),
            Slot::ActorReference(id) => Some(MolangValue::ActorReference(id)),
        }
    }

    /// Borrows a slot's value for comparison; `None` when unassigned.
    fn view(&self, slot: Slot) -> Option<SlotView<'_>> {
        match slot {
            Slot::Unset => None,
            Slot::Number(value) => Some(SlotView::Number(value)),
            Slot::String(index) => self.strings[index as usize]
                .as_deref()
                .map(SlotView::String),
            Slot::ActorReference(id) => Some(SlotView::ActorReference(id)),
        }
    }

    fn slot(&self, place: Place) -> Option<Slot> {
        match place {
            Place::Variable(slot) => self.values.get(slot).copied(),
            Place::Temporary(slot) => self.temps.get(slot).copied(),
        }
    }

    fn slot_mut(&mut self, place: Place) -> Option<&mut Slot> {
        match place {
            Place::Variable(slot) => self.values.get_mut(slot),
            Place::Temporary(slot) => self.temps.get_mut(slot),
        }
    }

    /// Assigns `value` (or unassigns) at `place`, keeping the string table owned one entry per
    /// string slot. Returns whether the slot was unassigned before; `None` when out of range.
    fn put(&mut self, place: Place, value: Option<MolangValue>) -> Option<bool> {
        let old = self.slot(place)?;
        if self.rollback.is_some() {
            let value = self.load(old);
            self.rollback.as_mut().unwrap().push((place, value));
        }
        #[cfg(test)]
        super::selection_work::record(|work| work.assigned += 1);
        let new = match value {
            None => Slot::Unset,
            Some(MolangValue::Number(value)) => Slot::Number(value),
            Some(MolangValue::ActorReference(id)) => Slot::ActorReference(id),
            Some(MolangValue::String(text)) => Slot::String(self.hold_string(old, text)),
        };
        if let Slot::String(index) = old
            && !matches!(new, Slot::String(_))
        {
            self.release_string(index);
        }
        *self.slot_mut(place)? = new;
        Some(matches!(old, Slot::Unset))
    }

    /// Stores `text` in the entry `old` already owns, a released entry, or a new one.
    fn hold_string(&mut self, old: Slot, text: Arc<str>) -> u32 {
        let index = match old {
            Slot::String(index) => index,
            _ => match self.free_strings.pop() {
                Some(index) => index,
                None => {
                    self.strings.push(None);
                    (self.strings.len() - 1) as u32
                }
            },
        };
        self.strings[index as usize] = Some(text);
        index
    }

    fn release_string(&mut self, index: u32) {
        self.strings[index as usize] = None;
        self.free_strings.push(index);
    }

    /// The value at `place`; `None` when unassigned or out of range.
    pub(super) fn read(&self, place: Place) -> Option<MolangValue> {
        self.slot(place).and_then(|slot| self.load(slot))
    }

    /// The assigned value of variable `slot`, if any.
    pub(super) fn variable(&self, slot: usize) -> Option<MolangValue> {
        self.read(Place::Variable(slot))
    }

    /// Every assigned variable as `(slot, value)`, in slot order.
    pub(super) fn assigned_variables(&self) -> impl Iterator<Item = (usize, MolangValue)> + '_ {
        self.values
            .iter()
            .enumerate()
            .inspect(|_| {
                #[cfg(test)]
                super::selection_work::record(|work| work.inherited += 1);
            })
            .filter_map(|(slot, value)| Some((slot, self.load(*value)?)))
    }

    /// Assigns variable `slot` without recording an authored write; out of range is ignored.
    pub(super) fn assign_variable(&mut self, slot: usize, value: MolangValue) {
        self.put(Place::Variable(slot), Some(value));
    }

    /// Nested recording preserves the enclosing authored-write bitmap.
    pub(crate) fn begin_effects(&mut self) -> EffectsCapture {
        let capture = EffectsCapture {
            enabled: self.capture_writes,
            writes: std::mem::take(&mut self.writes),
            random_draws: self.random_draws,
            random: self.random,
        };
        self.capture_writes = true;
        capture
    }

    /// Completed events restore their enclosing capture after retaining their effects.
    pub(crate) fn finish_effects(&mut self, capture: EffectsCapture) -> MolangEffects {
        let mut effects = MolangEffects {
            random_draws: self.random_draws.wrapping_sub(capture.random_draws),
            random_start: capture.random,
            random_end: self.random,
            ..Default::default()
        };
        for (word, &bits) in self.writes.iter().enumerate() {
            let mut bits = bits;
            while bits != 0 {
                let slot = word * u64::BITS as usize + bits.trailing_zeros() as usize;
                bits &= bits - 1;
                let (place, value) = if slot < self.values.len() {
                    (Place::Variable(slot), self.values[slot])
                } else {
                    let slot = slot - self.values.len();
                    (Place::Temporary(slot), self.temps[slot])
                };
                effects.writes.push((place, self.load(value)));
            }
        }
        let mut writes = capture.writes;
        if capture.enabled {
            writes.resize(writes.len().max(self.writes.len()), 0);
            for (target, bits) in writes.iter_mut().zip(&self.writes) {
                *target |= bits;
            }
        }
        self.writes = writes;
        self.capture_writes = capture.enabled;
        effects
    }

    /// Records authored assignments after endpoint pre-animation without copying frozen inputs.
    pub(crate) fn capture_writes(&mut self) {
        self.capture_writes = true;
        self.writes.clear();
        self.publication_random = Some((self.random, self.random_draws));
    }

    /// Publishes authored side effects to matching slots without replacing untouched values; random draws continue on the same stream.
    pub(crate) fn publish_writes(&self, target: &mut Self) {
        if let Some((random, draws)) = self.publication_random {
            target.advance_random(random, self.random, self.random_draws.wrapping_sub(draws));
        }
        for (word, &bits) in self.writes.iter().enumerate() {
            let mut bits = bits;
            while bits != 0 {
                let slot = word * u64::BITS as usize + bits.trailing_zeros() as usize;
                bits &= bits - 1;
                if slot < self.values.len() {
                    target.put(Place::Variable(slot), self.load(self.values[slot]));
                } else {
                    let slot = slot - self.values.len();
                    let value = self.load(self.temps[slot]);
                    let assigned = value.is_some();
                    if target.put(Place::Temporary(slot), value) == Some(true) && assigned {
                        target.live_temps.push(slot);
                    }
                }
            }
        }
    }

    pub(super) fn store(&mut self, place: Place, value: MolangValue) -> Result<(), EvalError> {
        let unset = self.put(place, Some(value)).ok_or(EvalError::Invalid)?;
        let slot = match place {
            Place::Variable(slot) => slot,
            Place::Temporary(slot) => {
                if unset {
                    self.live_temps.push(slot);
                }
                self.values.len() + slot
            }
        };
        self.record_write(slot);
        Ok(())
    }

    /// Unassigns `place` without recording an authored write.
    pub(super) fn unset(&mut self, place: Place) -> Result<(), EvalError> {
        self.put(place, None).map(drop).ok_or(EvalError::Invalid)
    }

    /// Tracks authored writes across both direct and mapped script slots.
    fn record_write(&mut self, slot: usize) {
        if self.capture_writes {
            let word = slot / u64::BITS as usize;
            if self.writes.len() <= word {
                self.writes.resize(word + 1, 0);
            }
            self.writes[word] |= 1 << (slot % u64::BITS as usize);
        }
    }

    /// Mapped scripts inherit write recording without retaining an earlier invocation's writes.
    pub(crate) fn inherit_write_capture(&mut self, source: &Self) {
        self.capture_writes = source.capture_writes;
        self.writes.clear();
        self.publication_random = None;
    }

    /// Mapped copies preserve the source script's authored-write identities.
    pub(crate) fn copy_named_from(
        &mut self,
        target: &[assets::MolangSymbol],
        source: &[assets::MolangSymbol],
        variables: &Self,
    ) {
        let first = source.partition_point(|symbol| symbol.kind < MolangSymbolKind::Variable);
        let target_first =
            target.partition_point(|symbol| symbol.kind < MolangSymbolKind::Variable);
        let target_end = target.partition_point(|symbol| symbol.kind <= MolangSymbolKind::Variable);
        for (offset, value) in variables.assigned_variables() {
            let Some(symbol) = source.get(first + offset) else {
                break;
            };
            if let Ok(slot) = target[target_first..target_end]
                .binary_search_by(|candidate| candidate.identifier.cmp(&symbol.identifier))
            {
                self.put(Place::Variable(slot), Some(value));
                if variables.capture_writes
                    && variables
                        .writes
                        .get(offset / u64::BITS as usize)
                        .is_some_and(|bits| bits & (1 << (offset % u64::BITS as usize)) != 0)
                {
                    self.record_write(slot);
                }
            }
        }
    }

    /// Assigns a string, keeping the stored copy when it already reads `value`.
    pub(crate) fn set_string(&mut self, slot: Option<usize>, value: &str) {
        let Some(slot) = slot else {
            return;
        };
        if let Some(Slot::String(index)) = self.values.get(slot).copied()
            && self.strings[index as usize].as_deref() == Some(value)
        {
            return;
        }
        self.put(
            Place::Variable(slot),
            Some(MolangValue::String(Arc::from(value))),
        );
    }

    pub(crate) fn set_actor_reference(&mut self, slot: Option<usize>, runtime_id: u64) {
        if let Some(slot) = slot {
            self.put(
                Place::Variable(slot),
                Some(MolangValue::ActorReference(runtime_id)),
            );
        }
    }

    pub(crate) fn set(&mut self, slot: Option<usize>, value: f32) {
        if let Some(slot) = slot {
            self.put(Place::Variable(slot), Some(MolangValue::Number(value)));
        }
    }

    pub(crate) fn clear(&mut self, slot: Option<usize>) {
        if let Some(slot) = slot {
            self.put(Place::Variable(slot), None);
        }
    }

    pub(crate) fn get(&self, slot: Option<usize>) -> Option<f32> {
        match *self.values.get(slot?)? {
            Slot::Unset => None,
            Slot::Number(value) => Some(value),
            Slot::String(_) | Slot::ActorReference(_) => Some(0.0),
        }
    }

    /// Compares persistent authored inputs, including assignment presence and value type.
    pub(crate) fn read_changed(
        &self,
        completed: &Self,
        layout: &VariableLayout,
        symbol: u32,
    ) -> bool {
        match layout.place(symbol) {
            Some(Place::Variable(slot)) => {
                self.values.get(slot).map(|value| self.view(*value))
                    != completed
                        .values
                        .get(slot)
                        .map(|value| completed.view(*value))
            }
            Some(Place::Temporary(_)) | None => false,
        }
    }

    /// Unassigns every temporary; only slots assigned since the last clear are visited.
    pub(crate) fn clear_temporaries(&mut self) {
        #[cfg(test)]
        super::selection_work::record(|work| work.reset += self.live_temps.len());
        let mut live = std::mem::take(&mut self.live_temps);
        for slot in live.drain(..) {
            self.put(Place::Temporary(slot), None);
        }
        self.live_temps = live;
    }

    #[cfg(test)]
    pub(crate) fn slots(count: usize) -> Self {
        Self {
            values: vec![Slot::Unset; count],
            temps: Vec::new(),
            random: 1,
            ..Default::default()
        }
    }

    #[cfg(test)]
    pub(crate) fn number_at(&self, slot: usize) -> Option<f32> {
        self.variable(slot).map(|value| value.number())
    }

    /// Effects consume draws after the receiving prefix, preserving its conditional random work.
    pub(super) fn advance_random(&mut self, start: u64, end: u64, draws: u64) {
        if self.random == start {
            self.random = end;
            self.random_draws = self.random_draws.wrapping_add(draws);
        } else {
            for _ in 0..draws {
                self.next_random();
            }
        }
    }

    /// Next value in `[0, 1]` from a per-actor xorshift stream.
    pub(super) fn next_random(&mut self) -> f32 {
        let mut state = self.random;
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        self.random = state;
        self.random_draws = self.random_draws.wrapping_add(1);
        (state >> 40) as f32 / (1_u64 << 24) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observed_predicates_restore_many_temporaries_with_linear_slot_work() {
        for count in [1, 100, 1_000] {
            let layout = VariableLayout {
                variable_count: 1,
                temp_count: count + 2,
                ..Default::default()
            };
            let mut variables = layout.fresh(7);
            variables
                .store(
                    Place::Temporary(0),
                    MolangValue::String(Arc::from("retained")),
                )
                .unwrap();
            variables
                .store(Place::Temporary(1), MolangValue::ActorReference(42))
                .unwrap();
            for _ in 0..3 {
                let mut observed = ObservedInputs::new(&variables);
                for slot in 0..count + 1 {
                    let place = Place::Temporary(slot);
                    observed.written(slot as u32, place, &variables);
                    variables
                        .store(place, MolangValue::String(Arc::from("scratch")))
                        .unwrap();
                }
                // The untouched final slot checks that cleanup does not scan the whole layout.
                super::super::selection_work::take();
                observed.restore(&mut variables);
                let work = super::super::selection_work::take();
                assert!(
                    work.restored <= 2 * (count + 1),
                    "count={count}, work={work:?}"
                );
                assert_eq!(
                    variables.read(Place::Temporary(0)),
                    Some(MolangValue::String(Arc::from("retained")))
                );
                assert_eq!(
                    variables.read(Place::Temporary(1)),
                    Some(MolangValue::ActorReference(42))
                );
                assert_eq!(variables.live_temps, [0, 1]);
                assert_eq!(variables.strings.iter().flatten().count(), 1);
                for slot in 2..count + 2 {
                    assert_eq!(variables.read(Place::Temporary(slot)), None);
                }
            }
            variables.clear_temporaries();
            assert!(variables.live_temps.is_empty());
            assert!(variables.strings.iter().all(Option::is_none));
        }
    }

    #[test]
    fn observed_predicates_restore_strings_references_and_sparse_temporaries() {
        let layout = VariableLayout {
            variable_count: 2,
            temp_count: 1,
            ..Default::default()
        };
        let mut variables = layout.fresh(7);
        variables.set_string(Some(0), "owner");
        variables.set_actor_reference(Some(1), 42);
        for _ in 0..3 {
            let mut observed = ObservedInputs::new(&variables);
            for (symbol, place) in [
                (0, Place::Variable(0)),
                (1, Place::Variable(1)),
                (2, Place::Temporary(0)),
            ] {
                observed.written(symbol, place, &variables);
                variables
                    .store(place, MolangValue::String(Arc::from("scratch")))
                    .unwrap();
            }
            variables.next_random();
            observed.restore(&mut variables);
            assert_eq!(
                variables.variable(0),
                Some(MolangValue::String(Arc::from("owner")))
            );
            assert_eq!(variables.variable(1), Some(MolangValue::ActorReference(42)));
            assert_eq!(variables.read(Place::Temporary(0)), None);
            assert!(variables.live_temps.is_empty());
            assert_eq!(variables.strings.iter().flatten().count(), 1);
            assert_eq!(variables.random_state(), 7);
            assert_eq!(variables.random_draws, 0);
        }
        variables.reset(8);
        assert_eq!(variables.variable(0), None);
        assert_eq!(variables.variable(1), None);
        assert_eq!(variables.read(Place::Temporary(0)), None);
        assert!(variables.strings.is_empty());
        variables.set_string(Some(0), "next owner");
        assert_eq!(
            variables.variable(0),
            Some(MolangValue::String(Arc::from("next owner")))
        );
        assert_eq!(variables.random_state(), 9);
    }

    #[test]
    fn strings_keep_one_entry_per_slot_and_copies_compare_by_content() {
        let mut variables = MolangVariables::slots(3);
        variables.set_string(Some(0), "a");
        variables.set_string(Some(1), "b");
        variables.set_string(Some(0), "c");
        variables.set(Some(1), 2.0);
        variables.set_string(Some(2), "d");
        assert_eq!(variables.strings.iter().flatten().count(), 2);
        assert_eq!(variables.strings.len(), 2, "the released entry is reused");
        assert_eq!(
            variables.variable(0),
            Some(MolangValue::String(Arc::from("c")))
        );
        assert_eq!(variables.get(Some(1)), Some(2.0));
        assert_eq!(
            variables.variable(2),
            Some(MolangValue::String(Arc::from("d")))
        );
        let mut other = MolangVariables::slots(3);
        other.set_string(Some(2), "c");
        other.set_string(Some(0), "c");
        other.set(Some(1), 2.0);
        other.set_string(Some(2), "d");
        let layout = VariableLayout {
            variable_count: 3,
            ..Default::default()
        };
        for symbol in 0..3 {
            assert!(!variables.read_changed(&other, &layout, symbol));
        }
        let mut pool = frame::FrameVariables::default();
        let copy = pool.lease(&variables);
        drop(copy);
        let mut pool = frame::FrameVariables::default();
        let copy = pool.lease(&other);
        assert_eq!(copy.variable(0), other.variable(0));
        assert_eq!(copy.variable(2), other.variable(2));
    }
}
