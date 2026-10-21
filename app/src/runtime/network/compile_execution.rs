//! Execution policy for independent pack compiler subscribers.

#[cfg(target_arch = "wasm32")]
/// Browser preparation runs inside the host's yielded finite job.
pub(super) struct Scope;
#[cfg(target_arch = "wasm32")]
impl Scope {
    /// Runs an independent subscriber before returning to its preparation owner.
    pub(super) fn spawn(&self, work: impl FnOnce(&Self)) {
        work(self);
    }
}
#[cfg(target_arch = "wasm32")]
/// Executes all subscribers within the current preparation job.
pub(super) fn scope<R>(work: impl FnOnce(&Scope) -> R) -> R {
    work(&Scope)
}
#[cfg(target_arch = "wasm32")]
/// Completes both independent preparations without creating native threads.
pub(super) fn join<A, B>(left: impl FnOnce() -> A, right: impl FnOnce() -> B) -> (A, B) {
    (left(), right())
}
