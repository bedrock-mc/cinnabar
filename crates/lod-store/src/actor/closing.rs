//! Waiting for a closing actor without blocking.

use std::{sync::mpsc::Receiver, thread::JoinHandle};

use super::ActorEvent;

/// A store actor's thread on its way down after [`super::StoreActor::close`]. The store's lock
/// is released once [`Self::is_finished`] is true, so polling it never blocks a frame.
#[derive(Debug)]
pub struct Closing {
    pub(super) thread: Option<JoinHandle<()>>,
    pub(super) events: Receiver<ActorEvent>,
}

impl Closing {
    /// True once the thread has closed the store and released its lock.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }

    /// Moves every queued event into `out`.
    pub fn poll_events(&mut self, out: &mut Vec<ActorEvent>) {
        out.extend(self.events.try_iter());
    }

    /// Waits for the thread and returns the events not yet polled.
    pub fn wait(mut self) -> Vec<ActorEvent> {
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        self.events.try_iter().collect()
    }
}
