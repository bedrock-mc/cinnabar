//! Cancellation-owned browser timers for relay deadlines and backoff.
use js_sys::Promise;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen(module = "/../site/edges.mjs")]
extern "C" {
    pub(super) fn milliseconds() -> f64;
    #[wasm_bindgen(js_name = startTimer)]
    fn start_timer(ms: u32) -> JsValue;
    #[wasm_bindgen(js_name = timerPromise)]
    fn timer_promise(handle: &JsValue) -> Promise;
    #[wasm_bindgen(js_name = cancelTimer)]
    fn cancel_timer(handle: &JsValue);
}

/// Holds the browser registration until completion or cancellation.
pub(super) struct Sleep {
    handle: JsValue,
    future: JsFuture,
}
impl Future for Sleep {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        Pin::new(&mut self.future).poll(cx).map(|_| ())
    }
}
impl Drop for Sleep {
    fn drop(&mut self) {
        cancel_timer(&self.handle);
    }
}

/// Registers a finite browser delay; dropping the future clears its timeout.
pub(super) fn sleep(duration: Duration) -> Sleep {
    let handle = start_timer(duration.as_millis().min(u128::from(u32::MAX)) as u32);
    let future = JsFuture::from(timer_promise(&handle));
    Sleep { handle, future }
}
