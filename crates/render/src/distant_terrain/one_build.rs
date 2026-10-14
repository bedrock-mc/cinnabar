//! Builds the renderer's GPU objects on a thread of their own, one build at a time, so a
//! burst of settings changes never holds more than one pool being built.

use std::sync::{
    Mutex, PoisonError,
    mpsc::{Receiver, TryRecvError},
};

/// A build running on its thread.
struct Running<K, T> {
    key: K,
    /// Behind a mutex only so the owner is `Sync`.
    receiver: Mutex<Receiver<T>>,
}

/// What [`OneBuild::poll`] found.
pub(crate) enum Polled<T> {
    /// The object built for the key still wanted; it is handed over once.
    Ready(T),
    /// A build runs, or nothing is wanted.
    Pending,
    /// The build thread could not start or ended without an object.
    Failed,
}

/// At most one build in flight, for the latest key asked for: a key asked for while
/// another builds starts once that one finished, and an object built for a key no longer
/// wanted is dropped.
pub(crate) struct OneBuild<K, T> {
    running: Option<Running<K, T>>,
    wanted: Option<K>,
}

impl<K, T> Default for OneBuild<K, T> {
    fn default() -> Self {
        Self {
            running: None,
            wanted: None,
        }
    }
}

impl<K: Copy + PartialEq, T: Send + 'static> OneBuild<K, T> {
    /// Asks for `key`'s object, or for none.
    pub(crate) fn want(&mut self, key: Option<K>) {
        self.wanted = key;
    }

    /// True while a build thread runs, wanted or not.
    pub(crate) fn running(&self) -> bool {
        self.running.is_some()
    }

    /// Takes a finished build, drops it unless still wanted, and starts the wanted build
    /// when none runs: `start` makes the work for a key, which runs on a thread of its own.
    pub(crate) fn poll<F>(&mut self, start: impl FnOnce(K) -> F) -> Polled<T>
    where
        F: FnOnce() -> T + Send + 'static,
    {
        if let Some(running) = &self.running {
            let received = running
                .receiver
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .try_recv();
            let key = running.key;
            match received {
                Err(TryRecvError::Empty) => return Polled::Pending,
                Err(TryRecvError::Disconnected) => {
                    self.running = None;
                    return Polled::Failed;
                }
                Ok(object) => {
                    self.running = None;
                    if self.wanted == Some(key) {
                        self.wanted = None;
                        return Polled::Ready(object);
                    }
                }
            }
        }
        let Some(key) = self.wanted else {
            return Polled::Pending;
        };
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let work = start(key);
        let spawned = std::thread::Builder::new()
            .name("lod-pipelines".to_owned())
            .spawn(move || {
                let _ = sender.send(work());
            });
        match spawned {
            Ok(_) => {
                self.running = Some(Running {
                    key,
                    receiver: Mutex::new(receiver),
                });
                Polled::Pending
            }
            Err(error) => {
                bevy::log::warn!(%error, "distant terrain build thread could not start");
                self.wanted = None;
                Polled::Failed
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc::{Sender, channel},
    };

    use test_time::eventually;

    use super::*;

    /// Starts builds that finish only once the test sends their key, counting starts.
    struct Gate {
        started: Arc<AtomicUsize>,
        release: Vec<Sender<()>>,
    }

    impl Gate {
        fn start(&mut self, key: u32) -> impl FnOnce() -> u32 + Send + 'static {
            let (sender, receiver) = channel::<()>();
            self.release.push(sender);
            self.started.fetch_add(1, Ordering::SeqCst);
            move || {
                let _ = receiver.recv();
                key
            }
        }
    }

    /// Polls until something other than [`Polled::Pending`] comes back.
    fn settle(builds: &mut OneBuild<u32, u32>, gate: &mut Gate) -> Option<u32> {
        let mut ready = None;
        eventually("the build to finish", || {
            match builds.poll(|key| gate.start(key)) {
                Polled::Ready(object) => {
                    ready = Some(object);
                    true
                }
                Polled::Pending => !builds.running(),
                Polled::Failed => panic!("the build failed"),
            }
        });
        ready
    }

    #[test]
    fn keys_asked_for_while_one_builds_wait_and_only_the_latest_is_built() {
        let mut gate = Gate {
            started: Arc::new(AtomicUsize::new(0)),
            release: Vec::new(),
        };
        let mut builds = OneBuild::<u32, u32>::default();
        builds.want(Some(1));
        assert!(matches!(
            builds.poll(|key| gate.start(key)),
            Polled::Pending
        ));
        for key in 2..6 {
            builds.want(Some(key));
            assert!(matches!(
                builds.poll(|key| gate.start(key)),
                Polled::Pending
            ));
        }
        assert_eq!(
            gate.started.load(Ordering::SeqCst),
            1,
            "one build at a time"
        );

        // The first build finishes unwanted: dropped, and the latest key starts.
        gate.release[0].send(()).expect("the first build waits");
        eventually("the latest build to start", || {
            let _ = builds.poll(|key| gate.start(key));
            gate.started.load(Ordering::SeqCst) == 2
        });
        gate.release[1].send(()).expect("the second build waits");
        assert_eq!(settle(&mut builds, &mut gate), Some(5));
        assert_eq!(gate.started.load(Ordering::SeqCst), 2);
        assert!(!builds.running());
        assert!(
            matches!(builds.poll(|key| gate.start(key)), Polled::Pending),
            "a handed-over object is not built again"
        );

        // Turning off while a build runs lets it finish and drops its object.
        builds.want(Some(7));
        let _ = builds.poll(|key| gate.start(key));
        builds.want(None);
        gate.release[2].send(()).expect("the third build waits");
        assert_eq!(settle(&mut builds, &mut gate), None);
        assert_eq!(gate.started.load(Ordering::SeqCst), 3);
    }
}
