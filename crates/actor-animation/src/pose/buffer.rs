//! Reusable local-channel scratch for frame samples and motion-history merging.
use super::*;
use std::{
    cell::RefCell,
    ops::{Deref, DerefMut},
};

thread_local! {
    static SPARE: RefCell<Vec<Vec<LocalDelta>>> = const { RefCell::new(Vec::new()) };
}

/// A sampled endpoint returns its local-channel storage after composition and skin mapping.
pub(crate) struct PoseBuffer(Vec<LocalDelta>);

impl PoseBuffer {
    /// Starts a clean endpoint while retaining the previous sample's allocation.
    pub(crate) fn lease(len: usize) -> Self {
        let mut local = take();
        local.resize(len, LocalDelta::default());
        Self(local)
    }

    /// Seeds an endpoint from retained channels without allocating a history clone.
    pub(crate) fn copy_of(source: &[LocalDelta]) -> Self {
        let mut local = take();
        local.extend_from_slice(source);
        Self(local)
    }

    /// Transfers storage into a completed tick that must retain its own history.
    pub(crate) fn into_vec(mut self) -> Vec<LocalDelta> {
        std::mem::take(&mut self.0)
    }
}
impl Deref for PoseBuffer {
    type Target = [LocalDelta];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl DerefMut for PoseBuffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl Drop for PoseBuffer {
    fn drop(&mut self) {
        let mut local = std::mem::take(&mut self.0);
        if local.capacity() == 0 {
            return;
        }
        local.clear();
        let _ = SPARE.try_with(|spare| spare.borrow_mut().push(local));
    }
}

/// Takes empty storage released by another endpoint on this sampling thread.
fn take() -> Vec<LocalDelta> {
    SPARE.with(|spare| spare.borrow_mut().pop().unwrap_or_default())
}
