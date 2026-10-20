//! Presentation traversal writes only the controller slots it touches.
use super::*;
use std::{
    cell::RefCell,
    ops::{Index, IndexMut},
};

thread_local! {
    static SPARE: RefCell<Vec<Vec<(usize, ControllerState)>>> = const { RefCell::new(Vec::new()) };
}

/// Tick traversal edits retained states; replay keeps a sparse, reusable overlay.
pub(crate) struct Controllers<'a> {
    source: &'a [ControllerState],
    retained: Option<&'a mut [ControllerState]>,
    edits: Vec<(usize, ControllerState)>,
}

impl<'a> Controllers<'a> {
    /// Mutates the tick's controller states in place.
    pub(crate) fn tick(states: &'a mut [ControllerState]) -> Self {
        Self {
            source: &[],
            retained: Some(states),
            edits: Vec::new(),
        }
    }

    /// Borrows frozen controllers and leases storage only for touched slots.
    pub(crate) fn replay(states: &'a [ControllerState]) -> Self {
        Self {
            source: states,
            retained: None,
            edits: SPARE.with(|spare| spare.borrow_mut().pop().unwrap_or_default()),
        }
    }

    /// Controller identity never changes in an overlay.
    pub(crate) fn iter(&self) -> std::slice::Iter<'_, ControllerState> {
        self.retained.as_deref().unwrap_or(self.source).iter()
    }
}
impl Index<usize> for Controllers<'_> {
    type Output = ControllerState;
    fn index(&self, index: usize) -> &Self::Output {
        if let Some(retained) = &self.retained {
            return &retained[index];
        }
        self.edits
            .iter()
            .find(|(slot, _)| *slot == index)
            .map_or(&self.source[index], |(_, state)| state)
    }
}
impl IndexMut<usize> for Controllers<'_> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        if let Some(retained) = &mut self.retained {
            return &mut retained[index];
        }
        let slot = match self.edits.iter().position(|(slot, _)| *slot == index) {
            Some(slot) => slot,
            None => {
                self.edits.push((index, self.source[index]));
                self.edits.len() - 1
            }
        };
        &mut self.edits[slot].1
    }
}
impl Drop for Controllers<'_> {
    fn drop(&mut self) {
        if self.retained.is_none() {
            let mut edits = std::mem::take(&mut self.edits);
            edits.clear();
            let _ = SPARE.try_with(|spare| spare.borrow_mut().push(edits));
        }
    }
}
