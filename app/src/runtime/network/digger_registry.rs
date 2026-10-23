//! Holds a replacement item registry at ingress while its tag programs compile.

use client_session::SequencedWorldEvent;
use crossbeam_channel::{Receiver, TryRecvError};
use pack_runtime::DiggerFilters;
use std::sync::Arc;

/// One bounded preparation, owned by the session that received the registry.
pub(crate) struct PendingRegistry {
    event: Option<SequencedWorldEvent>,
    result: Receiver<Arc<DiggerFilters>>,
    ready: Option<Arc<DiggerFilters>>,
}

impl PendingRegistry {
    /// Starts compilation without copying the registry or compiling on the frame thread.
    pub(super) fn start(event: SequencedWorldEvent, previous: Arc<DiggerFilters>) -> Self {
        Self::prepare(event, move |registry| {
            Arc::new(previous.replaced(registry.entries.iter()))
        })
    }

    /// Runs the registry compiler off the frame and retains its exact FIFO identity.
    fn prepare(
        event: SequencedWorldEvent,
        compile: impl FnOnce(&protocol::ItemRegistryEvent) -> Arc<DiggerFilters> + Send + 'static,
    ) -> Self {
        let protocol::WorldEvent::ItemActor(protocol::ItemActorEvent::Registry(registry)) =
            &event.event
        else {
            unreachable!("only item registries need digger preparation");
        };
        let registry = registry.clone();
        let (sender, result) = crossbeam_channel::bounded(1);
        let work = move || {
            let _ = sender.send(compile(&registry));
        };
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::spawn(work);
        #[cfg(target_arch = "wasm32")]
        crate::browser::jobs::item_registry(work);
        Self {
            event: Some(event),
            result,
            ready: None,
        }
    }

    /// Returns a complete registry and filter pair once its ingress lane admits it.
    /// Neither compiler completion nor admission ever waits on the frame thread.
    pub(super) fn poll(
        &mut self,
        admits: impl FnOnce(&SequencedWorldEvent) -> bool,
    ) -> Result<Option<(SequencedWorldEvent, Arc<DiggerFilters>)>, &'static str> {
        if self.ready.is_none() {
            match self.result.try_recv() {
                Ok(filters) => self.ready = Some(filters),
                Err(TryRecvError::Empty) => return Ok(None),
                Err(TryRecvError::Disconnected) => {
                    return Err("item registry preparation worker stopped");
                }
            }
        }
        let event = self.event.as_ref().expect("polled once when ready");
        if !admits(event) {
            return Ok(None);
        }
        Ok(Some((
            self.event.take().unwrap(),
            self.ready.take().unwrap(),
        )))
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use protocol::identity::FifoSequence;
    use simulation_clock::identity::SessionGeneration;

    /// Supplies a registry event with an observable session and FIFO identity.
    fn event() -> SequencedWorldEvent {
        SequencedWorldEvent {
            session_generation: SessionGeneration::new(7),
            sequence: FifoSequence::new(19),
            event: protocol::WorldEvent::ItemActor(protocol::ItemActorEvent::Registry(
                protocol::ItemRegistryEvent {
                    entries: Arc::new([]),
                },
            )),
        }
    }

    #[test]
    fn registry_compiler_runs_on_a_worker() {
        let (compiled, thread) = crossbeam_channel::bounded(1);
        let mut pending = PendingRegistry::prepare(event(), move |_| {
            compiled.send(std::thread::current().id()).unwrap();
            Arc::default()
        });
        let worker = thread.recv_timeout(test_time::DEFAULT_TIMEOUT).unwrap();
        assert_ne!(worker, std::thread::current().id());
        test_time::eventually("registry compiler finished", || {
            pending.poll(|_| true).unwrap().is_some()
        });
    }

    #[test]
    fn preparation_runs_off_frame_and_keeps_the_registry_pending_until_complete() {
        let frame = std::thread::current().id();
        let (started, running) = crossbeam_channel::bounded(1);
        let (release, wait) = crossbeam_channel::bounded(1);
        let expected = Arc::new(DiggerFilters::default());
        let prepared = Arc::clone(&expected);
        let mut pending = PendingRegistry::prepare(event(), move |_| {
            started.send(std::thread::current().id()).unwrap();
            wait.recv().unwrap();
            prepared
        });
        let worker = running.recv_timeout(test_time::DEFAULT_TIMEOUT).unwrap();
        assert_ne!(worker, frame);
        assert!(pending.poll(|_| true).unwrap().is_none());
        release.send(()).unwrap();
        let mut result = None;
        test_time::eventually("digger registry prepared", || {
            result = pending.poll(|_| true).unwrap();
            result.is_some()
        });
        let (event, filters) = result.unwrap();
        assert_eq!(event.session_generation.get(), 7);
        assert_eq!(event.sequence.get(), 19);
        assert!(Arc::ptr_eq(&expected, &filters));
    }

    #[test]
    fn completed_registry_waits_for_ingress_capacity() {
        let mut pending = PendingRegistry::start(event(), Arc::default());
        test_time::eventually("registry ready behind full ingress", || {
            assert!(pending.poll(|_| false).unwrap().is_none());
            pending.ready.is_some()
        });
        assert!(pending.poll(|_| false).unwrap().is_none());
        let (event, _) = pending.poll(|_| true).unwrap().unwrap();
        assert_eq!(event.sequence.get(), 19);
    }

    #[test]
    fn abandoned_registry_does_not_publish_into_a_replacement_session() {
        let (release, wait) = crossbeam_channel::bounded(1);
        let filters = Arc::new(DiggerFilters::default());
        let weak = Arc::downgrade(&filters);
        let pending = PendingRegistry::prepare(event(), move |_| {
            wait.recv().unwrap();
            filters
        });
        drop(pending);
        release.send(()).unwrap();
        test_time::eventually("abandoned digger filters released", || {
            weak.upgrade().is_none()
        });
    }
}
