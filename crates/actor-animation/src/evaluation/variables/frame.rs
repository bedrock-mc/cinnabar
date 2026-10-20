//! Scratch copies retained by their immutable tick endpoint.
use super::*;

/// Authoritative variables pay only for a pointer; frame-only state lives with its scratch copy.
#[derive(Debug, Default)]
pub(crate) struct ScratchState {
    pub(crate) cache: Option<crate::evaluation::frame_cache::Expressions>,
    pub(super) undo: Option<Vec<(Place, Option<MolangValue>)>>,
}

/// Scratch storage belongs to one actor's sampling cache and its outer frame lock.
#[derive(Debug, Default)]
pub(crate) struct FrameVariables(Vec<MolangVariables>);

impl FrameVariables {
    /// Replaces a tick baseline while retaining scratch allocations and expression plans.
    pub(crate) fn rebase(&mut self, source: &MolangVariables) {
        for variables in &mut self.0 {
            let frame = variables.frame.take();
            variables.clone_from(source);
            variables.frame = frame;
        }
    }

    /// Seeds storage from a tick baseline, then restores only writes between samples.
    pub(crate) fn lease<'a>(&'a mut self, source: &MolangVariables) -> FrameVariableLease<'a> {
        let mut variables = self.0.pop().unwrap_or_else(|| source.clone());
        let frame = variables.frame.get_or_insert_with(Default::default);
        frame.cache.get_or_insert_with(Default::default);
        frame.undo.get_or_insert_with(Vec::new).clear();
        let random = (variables.random, variables.random_draws);
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
    pool: &'a mut FrameVariables,
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
        let mut undo = variables.frame.as_mut().unwrap().undo.take().unwrap();
        for (place, value) in undo.drain(..).rev() {
            variables.put(place, value);
        }
        variables.live_temps.clear();
        variables.random = self.random.0;
        variables.random_draws = self.random.1;
        variables.capture_writes = false;
        variables.writes.clear();
        variables.publication_random = None;
        variables.frame.as_mut().unwrap().undo = Some(undo);
        self.pool.0.push(variables);
    }
}

thread_local! {
    static ISOLATION: RefCell<Vec<IsolationBuffers>> = const { RefCell::new(Vec::new()) };
}

#[derive(Default)]
struct IsolationBuffers {
    frame: Box<ScratchState>,
    writes: Vec<u64>,
}

/// Restores a layer's writes without copying its owner's variable or temporary tables.
pub(crate) struct Isolation<'a> {
    variables: &'a mut MolangVariables,
    outer_frame: Option<Box<ScratchState>>,
    outer_writes: Vec<u64>,
    random: (u64, u64),
    publication: Option<(u64, u64)>,
    capture: bool,
}

impl MolangVariables {
    /// Isolates one layer while reusing storage for its small write journal.
    pub(crate) fn isolate(&mut self) -> Isolation<'_> {
        let mut buffers = ISOLATION.with(|spare| spare.borrow_mut().pop().unwrap_or_default());
        let mut outer_frame = self.frame.take();
        if let Some(outer) = &mut outer_frame {
            buffers.frame.cache = outer.cache.take();
        }
        buffers.frame.undo.get_or_insert_with(Vec::new).clear();
        self.frame = Some(buffers.frame);
        let outer_writes = std::mem::replace(&mut self.writes, buffers.writes);
        Isolation {
            random: (self.random, self.random_draws),
            publication: self.publication_random,
            capture: self.capture_writes,
            variables: self,
            outer_frame,
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
        let mut frame = variables.frame.take().unwrap();
        let mut undo = frame.undo.take().unwrap();
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
        frame.undo = Some(undo);
        if let Some(outer) = &mut self.outer_frame {
            outer.cache = frame.cache.take();
        } else {
            frame.cache = None;
        }
        variables.frame = self.outer_frame.take();
        let _ =
            ISOLATION.try_with(|spare| spare.borrow_mut().push(IsolationBuffers { frame, writes }));
    }
}
