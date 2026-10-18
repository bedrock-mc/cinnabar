//! Browser ranges drive the existing sound-bank decoder and bounded PCM cache.
use assets::sound_bank::SoundBankIndex;
use client_audio::audio::{SoundBank, SoundDecodeDriver, SoundDecodeRequest};
use futures::{FutureExt, StreamExt, stream::FuturesUnordered};
use js_sys::{Promise, Uint8Array};
use std::{sync::Arc, time::Duration};
use wasm_bindgen::prelude::{JsCast, JsError, JsValue, wasm_bindgen};
use wasm_bindgen_futures::JsFuture;

use super::{game::Task, timer};

#[wasm_bindgen(module = "/../site/audio.mjs")]
extern "C" {
    #[wasm_bindgen(js_name = startRange)]
    fn start_range(source: &JsValue, offset: f64, bytes: u32) -> JsValue;
    #[wasm_bindgen(js_name = rangePromise)]
    fn range_promise(handle: &JsValue) -> Promise;
    #[wasm_bindgen(js_name = cancelRange)]
    fn cancel_range(handle: &JsValue);
}

/// Verified bank index paired with an HTTPS source pinned to one strong entity tag.
#[wasm_bindgen]
pub struct SoundSource {
    index: SoundBankIndex,
    source: JsValue,
}

#[wasm_bindgen]
impl SoundSource {
    /// Validates the prefix hash, every entry extent and the server's declared total size.
    #[wasm_bindgen(constructor)]
    pub fn new(prefix: &[u8], total: f64, source: JsValue) -> Result<Self, JsError> {
        let index = SoundBankIndex::decode_prefix(prefix)
            .map_err(|error| JsError::new(&error.to_string()))?;
        if !total.is_finite()
            || total > 9_007_199_254_740_991.0
            || total != index.total_bytes() as f64
        {
            return Err(JsError::new("sound bank length mismatch"));
        }
        Ok(Self { index, source })
    }
}

impl SoundSource {
    /// Installs bounded range work while keeping routing and decoding in their existing owners.
    pub(super) fn start(
        self,
        catalog: Option<Arc<assets::RuntimeAudioCatalog>>,
    ) -> (SoundBank, Task) {
        let (mut bank, driver) = SoundBank::with_range_source(self.index, catalog);
        bank.prewarm_common();
        (bank, Task::spawn(drive(driver, self.source)))
    }
}

/// Owns an abortable HTTP read until completion, pack replacement or page disposal.
struct Range(JsValue);
impl Drop for Range {
    fn drop(&mut self) {
        cancel_range(&self.0);
    }
}

/// Polls completions while the decoder's own semaphore limits admitted concurrent work.
async fn drive(mut driver: SoundDecodeDriver, source: JsValue) {
    let mut pending = FuturesUnordered::new();
    loop {
        let request = {
            let next = driver.next().fuse();
            let done = async {
                if pending.is_empty() {
                    futures::future::pending().await
                } else {
                    pending.next().await
                }
            }
            .fuse();
            futures::pin_mut!(next, done);
            futures::select! {
                request = next => Some(request),
                _ = done => None,
            }
        };
        if let Some(request) = request {
            let Some(request) = request else {
                return;
            };
            pending.push(decode(request, source.clone()));
        }
    }
}

/// Cancels obsolete reads before decode and reports every admitted request's result.
async fn decode(request: SoundDecodeRequest, source: JsValue) {
    let bytes = if let Some(entry) = request.range() {
        let range = Range(start_range(&source, entry.offset as f64, entry.len));
        let result = JsFuture::from(range_promise(&range.0)).fuse();
        futures::pin_mut!(result);
        loop {
            if !request.is_current() {
                return;
            }
            let changed = timer::sleep(Duration::from_millis(50)).fuse();
            futures::pin_mut!(changed);
            futures::select! {
                value = result => break value.ok().and_then(|value| value.dyn_into::<Uint8Array>().ok()).map(|value| value.to_vec()),
                _ = changed => {},
            }
        }
    } else {
        None
    };
    // Yield before finite PCM decode so network completions do not run in the frame callback.
    timer::sleep(Duration::ZERO).await;
    request.complete(bytes).await;
}

/// The canonical sound-bank output name, shared with asset compilation.
#[wasm_bindgen]
pub fn sound_bank_filename() -> String {
    assets::carriers::AUDIO_BANK.output.into()
}

/// Bytes needed to read the bank's bounded prefix length.
#[wasm_bindgen]
pub fn sound_bank_header_bytes() -> usize {
    assets::sound_bank::HEADER_BYTES
}

/// Validates the bank header before the page allocates its index buffer.
#[wasm_bindgen]
pub fn sound_bank_prefix_bytes(header: &[u8]) -> Result<usize, JsError> {
    assets::sound_bank::sound_bank_prefix_len(header)
        .map_err(|error| JsError::new(&error.to_string()))
}
