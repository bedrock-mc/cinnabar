//! Bounded browser jobs for resource-pack reloads and item registry preparation.
use std::cell::RefCell;
type Job = Box<dyn FnOnce()>;
thread_local! {
    static RELOAD: RefCell<Option<Job>> = const { RefCell::new(None) };
    static ITEM_REGISTRY: RefCell<Option<Job>> = const { RefCell::new(None) };
}
/// Queues the one item registry held by ingress until its programs are ready.
pub(crate) fn item_registry(work: impl FnOnce() + 'static) {
    ITEM_REGISTRY.with_borrow_mut(|pending| *pending = Some(Box::new(work)));
}
/// Replaces an unstarted reload after its owner changes session or revision.
pub(crate) fn reload(work: impl FnOnce() + 'static) {
    RELOAD.with_borrow_mut(|pending| *pending = Some(Box::new(work)));
}
/// Executes at most one admitted preparation after the event loop yields.
pub(super) fn run_one() -> bool {
    let work = ITEM_REGISTRY
        .with_borrow_mut(Option::take)
        .or_else(|| RELOAD.with_borrow_mut(Option::take));
    if let Some(work) = work {
        work();
        true
    } else {
        false
    }
}
