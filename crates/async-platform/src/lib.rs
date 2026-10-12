//! The runtime services shared code needs on every target: spawning tasks, timers, offloading
//! blocking work and background jobs. Native targets run them on Tokio, threads and Rayon;
//! WebAssembly runs them on the browser's event loop, which has one thread, so spawned futures
//! need not be `Send` there and blocking work and background jobs run inline.

use std::fmt;
use std::future::Future;
use std::time::Duration;

/// A future that did not finish before its [`timeout`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Elapsed;

impl fmt::Display for Elapsed {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("deadline has elapsed")
    }
}

impl std::error::Error for Elapsed {}

/// Runs `future` until it finishes or `duration` passes, whichever is first.
pub async fn timeout<F: Future>(duration: Duration, future: F) -> Result<F::Output, Elapsed> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        tokio::time::timeout(duration, future)
            .await
            .map_err(|_| Elapsed)
    }
    #[cfg(target_arch = "wasm32")]
    {
        use futures::future::{Either, select};
        let future = std::pin::pin!(future);
        let deadline = std::pin::pin!(web::sleep(duration));
        match select(future, deadline).await {
            Either::Left((output, _)) => Ok(output),
            Either::Right(((), _)) => Err(Elapsed),
        }
    }
}

/// Waits for `duration`.
pub async fn sleep(duration: Duration) {
    #[cfg(not(target_arch = "wasm32"))]
    tokio::time::sleep(duration).await;
    #[cfg(target_arch = "wasm32")]
    web::sleep(duration).await;
}

/// Runs `future` in the background on the current runtime; native callers need a Tokio runtime.
#[cfg(not(target_arch = "wasm32"))]
pub fn spawn<F>(future: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    drop(tokio::spawn(future));
}

/// Runs `future` in the background on the browser's event loop.
#[cfg(target_arch = "wasm32")]
pub fn spawn<F>(future: F)
where
    F: Future<Output = ()> + 'static,
{
    wasm_bindgen_futures::spawn_local(future);
}

/// A spawned future that is cancelled when this handle drops.
pub struct Task {
    #[cfg(not(target_arch = "wasm32"))]
    handle: tokio::task::JoinHandle<()>,
    #[cfg(target_arch = "wasm32")]
    handle: futures::future::AbortHandle,
}

impl Drop for Task {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

/// Spawns `future` like [`spawn`] and returns the handle that cancels it.
#[cfg(not(target_arch = "wasm32"))]
pub fn spawn_task<F>(future: F) -> Task
where
    F: Future<Output = ()> + Send + 'static,
{
    Task {
        handle: tokio::spawn(future),
    }
}

/// Spawns `future` like [`spawn`] and returns the handle that cancels it.
#[cfg(target_arch = "wasm32")]
pub fn spawn_task<F>(future: F) -> Task
where
    F: Future<Output = ()> + 'static,
{
    let (handle, registration) = futures::future::AbortHandle::new_pair();
    spawn(async move {
        let _ = futures::future::Abortable::new(future, registration).await;
    });
    Task { handle }
}

/// Runs `work` off the async runtime: `Some` of its result or its panic, or `None` if the
/// runtime shut down first. WebAssembly has no other thread, so there it runs before this returns.
pub async fn run_blocking<T, F>(work: F) -> Option<std::thread::Result<T>>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    #[cfg(not(target_arch = "wasm32"))]
    {
        match tokio::task::spawn_blocking(work).await {
            Ok(output) => Some(Ok(output)),
            Err(error) if error.is_panic() => Some(Err(error.into_panic())),
            Err(_) => None,
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        Some(std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)))
    }
}

/// Runs `job` on a new thread named `name`; a browser, which cannot start threads, runs it before
/// returning. Callers that wait on its result must not block before it finishes.
pub fn spawn_thread(name: &str, job: impl FnOnce() + Send + 'static) -> std::io::Result<()> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::thread::Builder::new()
            .name(name.to_owned())
            .spawn(job)
            .map(drop)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = name;
        job();
        Ok(())
    }
}

/// Runs `job` on Rayon's global pool; a browser, whose pool has only the calling thread and runs
/// spawned work only when that thread next enters Rayon, runs it before returning.
pub fn spawn_pooled(job: impl FnOnce() + Send + 'static) {
    #[cfg(not(target_arch = "wasm32"))]
    rayon::spawn(job);
    #[cfg(target_arch = "wasm32")]
    job();
}

#[cfg(target_arch = "wasm32")]
mod web {
    use std::time::Duration;

    use wasm_bindgen::prelude::*;

    #[wasm_bindgen]
    extern "C" {
        /// The global `setTimeout`, present in windows and workers alike.
        #[wasm_bindgen(js_name = setTimeout)]
        fn set_timeout(callback: &js_sys::Function, milliseconds: i32) -> JsValue;
    }

    /// Resolves after `duration`, rounded up to whole milliseconds.
    pub(crate) async fn sleep(duration: Duration) {
        let milliseconds = duration
            .as_nanos()
            .div_ceil(1_000_000)
            .min(i32::MAX as u128) as i32;
        let promise = js_sys::Promise::new(&mut |resolve, _| {
            set_timeout(&resolve, milliseconds);
        });
        let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    #[tokio::test]
    async fn timeout_returns_the_output_of_a_ready_future() {
        assert_eq!(
            super::timeout(Duration::from_secs(1), async { 7 }).await,
            Ok(7)
        );
    }

    #[tokio::test]
    async fn timeout_elapses_for_a_future_that_never_finishes() {
        let never = std::future::pending::<()>();
        assert_eq!(
            super::timeout(Duration::from_millis(5), never).await,
            Err(super::Elapsed)
        );
    }

    #[test]
    fn spawned_jobs_run() {
        let (sender, receiver) = std::sync::mpsc::channel();
        let pooled = sender.clone();
        super::spawn_thread("test-job", move || sender.send(1).unwrap()).unwrap();
        super::spawn_pooled(move || pooled.send(2).unwrap());
        let mut received: Vec<i32> = receiver.iter().take(2).collect();
        received.sort_unstable();
        assert_eq!(received, [1, 2]);
    }

    #[tokio::test]
    async fn dropping_a_task_cancels_it() {
        let (_never, pending) = tokio::sync::oneshot::channel::<()>();
        let (finished, done) = tokio::sync::oneshot::channel::<()>();
        let task = super::spawn_task(async move {
            let _ = pending.await;
            let _ = finished.send(());
        });
        drop(task);
        assert!(done.await.is_err(), "a dropped task never finishes");
    }

    #[tokio::test]
    async fn run_blocking_returns_the_result_or_the_panic() {
        assert_eq!(super::run_blocking(|| 3).await.unwrap().unwrap(), 3);
        let panic = super::run_blocking(|| -> u8 { panic!("boom") })
            .await
            .unwrap();
        assert_eq!(*panic.unwrap_err().downcast::<&str>().unwrap(), "boom");
    }
}
