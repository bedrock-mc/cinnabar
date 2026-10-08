//! The store's link to the core: an API worker, so a slow purchase never queues behind image downloads,
//! and a pool of image workers fetching offer art in parallel; the render loop only ever polls a channel.

use std::{path::PathBuf, thread};

use bevy::prelude::Resource;
use crossbeam_channel::{Receiver, Sender, TrySendError, bounded, unbounded};
use protocol::store_control::{self, BridgeError};

const API_QUEUE: usize = 32;
const IMAGE_QUEUE: usize = 64;
/// Concurrent offer image fetches; each thumbnail is a few hundred KB from one CDN host.
const IMAGE_WORKERS: usize = 8;

pub(crate) use launcher::store::worker::{StoreError, StoreEvent, StoreRequest};

fn reduce<T>(result: Result<T, BridgeError>) -> Result<T, StoreError> {
    result.map_err(|error| StoreError::from(&error))
}

/// Handle to the store workers; they stop when this is dropped.
#[derive(Resource)]
pub(crate) struct StoreWorker {
    api: Sender<StoreRequest>,
    images: Sender<StoreRequest>,
    events: Receiver<StoreEvent>,
}

impl StoreWorker {
    /// Start the workers against the control endpoint under `socket_dir`.
    pub(crate) fn new(socket_dir: PathBuf) -> Self {
        let (api, api_requests) = bounded(API_QUEUE);
        let (images, image_requests) = bounded(IMAGE_QUEUE);
        let (event_tx, events) = unbounded();
        let pool = std::iter::repeat_n(image_requests, IMAGE_WORKERS);
        for requests in std::iter::once(api_requests).chain(pool) {
            let socket_dir = socket_dir.clone();
            let event_tx = event_tx.clone();
            thread::spawn(move || serve(&socket_dir, &requests, &event_tx));
        }
        Self {
            api,
            images,
            events,
        }
    }

    /// Queue a request; `false` when the queue is full or the worker is gone, so callers can retry later.
    pub(crate) fn send(&self, request: StoreRequest) -> bool {
        let queue = if matches!(request, StoreRequest::Image(_)) {
            &self.images
        } else {
            &self.api
        };
        match queue.try_send(request) {
            Ok(()) => true,
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => false,
        }
    }

    /// Everything the workers finished since the last poll.
    pub(crate) fn poll(&self) -> Vec<StoreEvent> {
        self.events.try_iter().collect()
    }
}

fn serve(
    socket_dir: &std::path::Path,
    requests: &Receiver<StoreRequest>,
    events: &Sender<StoreEvent>,
) {
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return;
    };
    while let Ok(request) = requests.recv() {
        let event = runtime.block_on(handle(socket_dir, request));
        if events.send(event).is_err() {
            return;
        }
    }
}

async fn handle(socket_dir: &std::path::Path, request: StoreRequest) -> StoreEvent {
    match request {
        StoreRequest::Home(page) => StoreEvent::Page(reduce(
            store_control::store_home(socket_dir, page.as_deref()).await,
        )),
        StoreRequest::Search(search) => StoreEvent::Search(reduce(
            store_control::store_search(socket_dir, &search).await,
        )),
        StoreRequest::Offer(id) => {
            match reduce(store_control::store_offer(socket_dir, &id).await) {
                Ok(detail) => StoreEvent::Offer(Ok(Box::new(detail))),
                Err(error) => StoreEvent::OfferFailed { id, error },
            }
        }
        StoreRequest::Balance => {
            StoreEvent::Balance(reduce(store_control::store_balance(socket_dir).await))
        }
        StoreRequest::Entitlements { offset, refresh } => StoreEvent::Entitlements {
            offset,
            result: reduce(store_control::store_entitlements(socket_dir, offset, 0, refresh).await),
        },
        StoreRequest::RowMore { row, continuation } => StoreEvent::RowMore {
            row,
            result: reduce(store_control::store_row_more(socket_dir, &continuation).await),
        },
        StoreRequest::Purchase(purchase) => StoreEvent::Purchase {
            purchase_id: purchase.purchase_id().to_owned(),
            result: reduce(store_control::store_purchase(socket_dir, &purchase).await),
        },
        StoreRequest::Image(url) => {
            let result = reduce(store_control::store_image(socket_dir, &url).await);
            StoreEvent::Image {
                url,
                result: result.map(|image| image.path),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rpc(code: i64) -> BridgeError {
        BridgeError::ControlRpc {
            code,
            message: String::new(),
        }
    }

    #[test]
    fn rpc_codes_reduce_to_screen_reactions() {
        assert_eq!(StoreError::from(&rpc(-32020)), StoreError::SignedOut);
        assert_eq!(StoreError::from(&rpc(-32031)), StoreError::Busy);
        for code in [-32602, -32032, -32033] {
            assert_eq!(StoreError::from(&rpc(code)), StoreError::Rejected);
        }
        assert_eq!(StoreError::from(&rpc(-32021)), StoreError::Unavailable);
        assert_eq!(
            StoreError::from(&BridgeError::ControlClosed),
            StoreError::Unavailable
        );
    }

    #[test]
    fn a_full_queue_refuses_instead_of_blocking() {
        let (api, _api_rx) = bounded(1);
        let (images, _image_rx) = bounded(1);
        let (_events_tx, events) = unbounded();
        let worker = StoreWorker {
            api,
            images,
            events,
        };
        assert!(worker.send(StoreRequest::Balance));
        assert!(!worker.send(StoreRequest::Balance));
        assert!(worker.send(StoreRequest::Image("https://x.test/a.png".into())));
        assert!(worker.poll().is_empty());
    }

    // One image worker fetched every thumbnail in turn, so a page took the sum of its downloads.
    #[cfg(unix)]
    #[test]
    fn image_requests_are_fetched_in_parallel() {
        use std::{io::Read, os::unix::net::UnixListener, time::Duration};
        let dir = std::env::temp_dir().join(format!("store-images-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let listener =
            UnixListener::bind(protocol::launcher_control::control_endpoint_path(&dir)).unwrap();
        let worker = StoreWorker::new(dir.clone());
        for index in 0..IMAGE_WORKERS {
            assert!(worker.send(StoreRequest::Image(format!("https://x.test/{index}.jpg"))));
        }
        // Each worker holds its connection open until answered; none is answered here.
        let mut held = Vec::new();
        listener.set_nonblocking(true).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while held.len() < IMAGE_WORKERS && std::time::Instant::now() < deadline {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false).unwrap();
                    let mut size = [0; 4];
                    stream.read_exact(&mut size).unwrap();
                    held.push(stream);
                }
                Err(_) => std::thread::sleep(Duration::from_millis(5)),
            }
        }
        let concurrent = held.len();
        drop(held);
        drop(worker);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(concurrent, IMAGE_WORKERS, "image fetches ran one at a time");
    }
}
