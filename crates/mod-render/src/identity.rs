//! One namespace for retained geometry, actor masks and their incarnation tokens.
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);
/// Allocates a process-unique drawing owner across every resource kind and producer.
pub(crate) fn next_owner() -> u64 {
    NEXT_OWNER
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |owner| {
            owner.checked_add(1)
        })
        .expect("drawing owners exhausted")
}
