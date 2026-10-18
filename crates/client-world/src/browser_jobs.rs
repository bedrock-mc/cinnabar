//! Event-loop execution for the shared pinned block-name indexes.
use assets::NetworkIdMode;
use std::{cell::RefCell, collections::VecDeque};

type Job = Box<dyn FnOnce() + Send>;
thread_local! {
    static REGISTRIES: RefCell<VecDeque<(NetworkIdMode, Job)>> = const { RefCell::new(VecDeque::new()) };
}

/// Admits the one initialization for each ID mode; the mode's OnceLock prevents repeats.
pub(crate) fn registry(mode: NetworkIdMode, work: Job) {
    REGISTRIES.with_borrow_mut(|pending| {
        assert!(
            !pending.iter().any(|(queued, _)| *queued == mode),
            "registry already queued"
        );
        pending.push_back((mode, work));
    });
}

/// Runs one finite registry load after the host yields, then publishes its immutable result.
pub fn run_one() -> bool {
    let work = REGISTRIES.with_borrow_mut(VecDeque::pop_front);
    if let Some((_, work)) = work {
        work();
        true
    } else {
        false
    }
}
