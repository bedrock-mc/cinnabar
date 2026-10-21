//! Browser work owned by the application's resource-pack reload state.
use std::cell::RefCell;
type Job = Box<dyn FnOnce()>;
thread_local! { static RELOAD: RefCell<Option<Job>> = const { RefCell::new(None) }; }
/// Replaces an unstarted reload after its owner changes session or revision.
pub(crate) fn reload(work: impl FnOnce() + 'static) {
    RELOAD.with_borrow_mut(|pending| *pending = Some(Box::new(work)));
}
/// Executes at most one admitted reload after the event loop yields.
pub(super) fn run_one() -> bool {
    let work = RELOAD.with_borrow_mut(Option::take);
    if let Some(work) = work {
        work();
        true
    } else {
        false
    }
}
