use super::*;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(module = "/../../web/site/dressing.mjs")]
extern "C" {
    #[wasm_bindgen(js_name = nextDressingFrame)]
    fn next_frame() -> js_sys::Promise;
}

/// Waits for another browser frame instead of blocking on an empty channel.
async fn yield_frame() {
    let _ = wasm_bindgen_futures::JsFuture::from(next_frame()).await;
}

/// Coalesces bounded gallery requests and decodes at most one visible source per task slice.
pub(super) fn start() -> (Sender<Request>, Receiver<Request>, Receiver<Packed>) {
    let (requests, jobs) = crossbeam_channel::bounded::<Request>(1);
    let (done, results) = crossbeam_channel::bounded(1);
    let pending = jobs.clone();
    wasm_bindgen_futures::spawn_local(async move {
        let mut cache = DecodeCache::default();
        let mut request: Option<Request> = None;
        loop {
            yield_frame().await;
            match jobs.try_recv() {
                Ok(newer) => request = Some(newer),
                Err(crossbeam_channel::TryRecvError::Disconnected) => return,
                Err(crossbeam_channel::TryRecvError::Empty) => {}
            }
            let Some(current) = &request else {
                continue;
            };
            let missing = cache.missing(&current.set);
            if let Some(source) = missing.first() {
                cache.decode(std::slice::from_ref(source), &current.set);
                continue;
            }
            let packed = pack(&current.set, &cache, current.id, true);
            match done.try_send(packed) {
                Ok(()) => {
                    cache.trim(&current.set);
                    request = None;
                }
                Err(crossbeam_channel::TrySendError::Disconnected(_)) => return,
                Err(crossbeam_channel::TrySendError::Full(_)) => {}
            }
        }
    });
    (requests, pending, results)
}
