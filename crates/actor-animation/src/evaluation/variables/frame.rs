//! Scratch copies retained by their immutable tick endpoint.
use super::*;
use std::sync::Mutex;

/// Each concurrent borrower leases its own buffers; clones start with an empty pool.
#[derive(Debug, Default)]
pub(crate) struct FrameVariables(Mutex<Vec<MolangVariables>>);

impl Clone for FrameVariables {
    /// A cloned endpoint owns a separate pool tied to its own frozen variable inputs.
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl FrameVariables {
    /// Replaces a tick baseline while retaining scratch allocations and expression plans.
    pub(crate) fn rebase(&mut self, source: &MolangVariables) {
        for variables in self.0.get_mut().unwrap() {
            let cache = variables.frame_cache.take();
            let undo = variables.rollback.take();
            variables.clone_from(source);
            variables.frame_cache = cache;
            variables.rollback = undo;
        }
    }

    /// Copies the immutable baseline once, then restores only writes between samples.
    pub(crate) fn lease<'a>(&'a self, source: &MolangVariables) -> FrameVariableLease<'a> {
        let mut variables = self
            .0
            .lock()
            .unwrap()
            .pop()
            .unwrap_or_else(|| source.clone());
        variables.frame_cache.get_or_insert_with(Default::default);
        let random = (variables.random, variables.random_draws);
        let mut undo = variables.rollback.take().unwrap_or_default();
        undo.clear();
        variables.rollback = Some(undo);
        FrameVariableLease {
            variables,
            pool: self,
            random,
        }
    }
}

/// Returns scratch storage and its original random stream on every exit path.
pub(crate) struct FrameVariableLease<'a> {
    variables: MolangVariables,
    pool: &'a FrameVariables,
    random: (u64, u64),
}

impl std::ops::Deref for FrameVariableLease<'_> {
    type Target = MolangVariables;
    fn deref(&self) -> &Self::Target {
        &self.variables
    }
}
impl std::ops::DerefMut for FrameVariableLease<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.variables
    }
}
impl Drop for FrameVariableLease<'_> {
    fn drop(&mut self) {
        let mut variables = std::mem::take(&mut self.variables);
        variables.clear_temporaries();
        let mut undo = variables.rollback.take().unwrap();
        for (place, value) in undo.drain(..).rev() {
            variables.put(place, value);
        }
        variables.live_temps.clear();
        variables.random = self.random.0;
        variables.random_draws = self.random.1;
        variables.capture_writes = false;
        variables.writes.clear();
        variables.publication_random = None;
        variables.rollback = Some(undo);
        self.pool.0.lock().unwrap().push(variables);
    }
}

thread_local! {
    static ISOLATION: RefCell<Vec<IsolationBuffers>> = const { RefCell::new(Vec::new()) };
}

#[derive(Default)]
struct IsolationBuffers {
    undo: Vec<(Place, Option<MolangValue>)>,
    writes: Vec<u64>,
}

/// Restores a layer's writes without copying its owner's variable or temporary tables.
pub(crate) struct Isolation<'a> {
    variables: &'a mut MolangVariables,
    outer_undo: Option<Vec<(Place, Option<MolangValue>)>>,
    outer_writes: Vec<u64>,
    random: (u64, u64),
    publication: Option<(u64, u64)>,
    capture: bool,
}

impl MolangVariables {
    /// Isolates one layer while reusing storage for its small write journal.
    pub(crate) fn isolate(&mut self) -> Isolation<'_> {
        let buffers = ISOLATION.with(|spare| spare.borrow_mut().pop().unwrap_or_default());
        let outer_undo = self.rollback.replace(buffers.undo);
        let outer_writes = std::mem::replace(&mut self.writes, buffers.writes);
        Isolation {
            random: (self.random, self.random_draws),
            publication: self.publication_random,
            capture: self.capture_writes,
            variables: self,
            outer_undo,
            outer_writes,
        }
    }
}
impl std::ops::Deref for Isolation<'_> {
    type Target = MolangVariables;
    fn deref(&self) -> &Self::Target {
        self.variables
    }
}
impl std::ops::DerefMut for Isolation<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.variables
    }
}
impl Drop for Isolation<'_> {
    fn drop(&mut self) {
        let variables = &mut *self.variables;
        let mut undo = variables.rollback.take().unwrap();
        for (place, value) in undo.drain(..).rev() {
            variables.put(place, value);
        }
        variables.live_temps.clear();
        variables.live_temps.extend(
            variables
                .temps
                .iter()
                .enumerate()
                .filter_map(|(slot, value)| (!matches!(value, Slot::Unset)).then_some(slot)),
        );
        variables.random = self.random.0;
        variables.random_draws = self.random.1;
        variables.publication_random = self.publication;
        variables.capture_writes = self.capture;
        let mut writes = std::mem::replace(
            &mut variables.writes,
            std::mem::take(&mut self.outer_writes),
        );
        writes.clear();
        variables.rollback = self.outer_undo.take();
        let _ =
            ISOLATION.try_with(|spare| spare.borrow_mut().push(IsolationBuffers { undo, writes }));
    }
}
