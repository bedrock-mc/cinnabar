//! Bounded event-loop work for session retirement and resource geometry preparation.
use std::{cell::RefCell, collections::VecDeque};
type Job = Box<dyn FnOnce()>;
const MAX_RETIREMENTS: usize = 2;
thread_local! {
    static GEOMETRY: RefCell<Option<Job>> = const { RefCell::new(None) };
    static RETIRED: RefCell<VecDeque<Job>> = const { RefCell::new(VecDeque::new()) };
}

/// Queues geometry preparation, superseding unstarted work abandoned by a session reset.
pub(crate) fn geometry(work: impl FnOnce() + 'static) {
    GEOMETRY.with_borrow_mut(|pending| {
        *pending = Some(Box::new(work));
    });
}

/// Retains at most two retired snapshots; pressure releases the oldest at this boundary.
pub(crate) fn retire(work: impl FnOnce() + 'static) {
    let oldest = RETIRED.with_borrow_mut(|pending| {
        let oldest = (pending.len() == MAX_RETIREMENTS)
            .then(|| pending.pop_front())
            .flatten();
        pending.push_back(Box::new(work));
        oldest
    });
    if let Some(oldest) = oldest {
        let started = web_time::Instant::now();
        oldest();
        bevy::log::warn!(
            elapsed_ms = started.elapsed().as_secs_f64() * 1000.0,
            "browser retirement queue full; released oldest snapshot on the frame"
        );
    }
}

/// Runs one finite retirement or geometry job after the host yields.
pub fn run_one() -> bool {
    let work = RETIRED
        .with_borrow_mut(VecDeque::pop_front)
        .or_else(|| GEOMETRY.with_borrow_mut(Option::take));
    if let Some(work) = work {
        work();
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    #[test]
    fn replacement_discards_unstarted_geometry_and_retirement_is_bounded() {
        let output = Rc::new(RefCell::new(Vec::new()));
        let old = output.clone();
        geometry(move || old.borrow_mut().push("old"));
        let fresh = output.clone();
        geometry(move || fresh.borrow_mut().push("geometry"));
        for _ in 0..=MAX_RETIREMENTS {
            let retired = output.clone();
            retire(move || retired.borrow_mut().push("retired"));
        }
        assert_eq!(&*output.borrow(), &["retired"]);
        assert!(run_one());
        assert!(run_one());
        assert!(run_one());
        assert!(!run_one());
        assert_eq!(
            &*output.borrow(),
            &["retired", "retired", "retired", "geometry"]
        );
    }
}
