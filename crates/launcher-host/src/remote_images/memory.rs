//! Remote form images (`http`/`https` button images) downloaded off the frame
//! loop into a bounded in-memory cache, as the vanilla client fetches them.
//! A few download at once, newest request first; unasked ones are dropped.

use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex, Weak},
    time::{Duration, Instant},
};

use tokio::sync::mpsc::{Receiver, Sender, error::TrySendError};

use super::{FORM_IMAGES, client, download};

/// Most URLs remembered, loading ones included; the oldest settled entry goes first.
const MAX_ENTRIES: usize = 64;
/// Requests waiting for a download slot; the oldest are dropped first.
const MAX_QUEUED: usize = MAX_ENTRIES;
/// Downloads in flight at once, so one slow host cannot hold up the rest.
const MAX_IN_FLIGHT: usize = 4;
/// A queued URL no screen asked for within this long is dropped unfetched.
const OBSOLETE_AFTER: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, PartialEq)]
pub enum RemoteState {
    Loading,
    Ready(Arc<[u8]>),
    Failed,
}

/// A shared handle to the download cache and its worker thread.
#[derive(Clone, Default)]
pub struct RemoteImages(Arc<Remote>);

#[derive(Default)]
struct Remote {
    entries: Mutex<Entries>,
    worker: Mutex<Option<Sender<String>>>,
}

#[derive(Default)]
struct Entries {
    states: HashMap<String, RemoteState>,
    order: VecDeque<String>,
    /// When a drawn screen last asked for each loading URL.
    asked: HashMap<String, Instant>,
}

impl Entries {
    /// Evicts the oldest settled URLs, then the oldest loading ones, down to [`MAX_ENTRIES`].
    fn trim(&mut self) {
        while self.order.len() > MAX_ENTRIES {
            let position = self
                .order
                .iter()
                .position(|key| self.states.get(key) != Some(&RemoteState::Loading))
                .unwrap_or(0);
            if let Some(key) = self.order.remove(position) {
                self.states.remove(&key);
                self.asked.remove(&key);
            }
        }
    }

    /// Forgets a loading URL so a later ask queues it afresh.
    fn forget(&mut self, url: &str) {
        self.asked.remove(url);
        if self.states.get(url) == Some(&RemoteState::Loading) {
            self.states.remove(url);
            self.order.retain(|key| key != url);
        }
    }
}

pub fn is_remote(path: &str) -> bool {
    path.starts_with("https://") || path.starts_with("http://")
}

impl RemoteImages {
    /// The download state of `url`, starting its download on first sight.
    pub fn state(&self, url: &str) -> RemoteState {
        let mut entries = self.0.entries.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(state) = entries.states.get(url).cloned() {
            if state == RemoteState::Loading {
                entries.asked.insert(url.to_owned(), Instant::now());
            }
            return state;
        }
        let valid =
            url::Url::parse(url).is_ok_and(|parsed| matches!(parsed.scheme(), "http" | "https"));
        let state = if !valid {
            RemoteState::Failed
        } else {
            match self.request(url) {
                Ok(()) => {
                    entries.asked.insert(url.to_owned(), Instant::now());
                    RemoteState::Loading
                }
                // A full queue is retried on the next ask rather than remembered as failed.
                Err(TrySendError::Full(_)) => return RemoteState::Loading,
                Err(TrySendError::Closed(_)) => RemoteState::Failed,
            }
        };
        entries.states.insert(url.to_owned(), state.clone());
        entries.order.push_back(url.to_owned());
        entries.trim();
        state
    }

    /// Queues a URL on the shared download worker.
    fn request(&self, url: &str) -> Result<(), TrySendError<String>> {
        let mut worker = self.0.worker.lock().unwrap_or_else(|p| p.into_inner());
        if worker.is_none() {
            *worker = spawn(Arc::downgrade(&self.0));
        }
        match worker.as_ref() {
            Some(sender) => sender.try_send(url.to_owned()),
            None => Err(TrySendError::Closed(url.to_owned())),
        }
    }
}

impl Remote {
    /// Settles a download and restores the retained cache capacity.
    fn finish(&self, url: String, bytes: Option<Vec<u8>>) {
        let mut entries = self.entries.lock().unwrap_or_else(|p| p.into_inner());
        entries.asked.remove(&url);
        if let Some(state) = entries.states.get_mut(&url) {
            *state = bytes.map_or(RemoteState::Failed, |bytes| {
                RemoteState::Ready(bytes.into())
            });
        }
        entries.trim();
    }

    /// Whether a queued `url` is still wanted; an obsolete one is forgotten so
    /// a later ask queues it afresh.
    fn still_wanted(&self, url: &str) -> bool {
        let mut entries = self.entries.lock().unwrap_or_else(|p| p.into_inner());
        let fresh = entries
            .asked
            .get(url)
            .is_some_and(|asked| asked.elapsed() < OBSOLETE_AFTER);
        if !fresh {
            entries.forget(url);
        }
        fresh
    }

    /// Drops a queued URL the worker had no room for.
    fn dropped(&self, url: &str) {
        self.entries
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .forget(url);
    }
}

/// One worker keeping up to [`MAX_IN_FLIGHT`] downloads going, newest request
/// first (what a screen just drew); it ends with the cache.
fn spawn(remote: Weak<Remote>) -> Option<Sender<String>> {
    let (sender, receiver) = tokio::sync::mpsc::channel::<String>(MAX_QUEUED);
    std::thread::Builder::new()
        .name("form-images".to_owned())
        .spawn(move || {
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            let Some(client) = client(&FORM_IMAGES) else {
                return;
            };
            runtime.block_on(serve(remote, receiver, client));
        })
        .ok()?;
    Some(sender)
}

async fn serve(remote: Weak<Remote>, mut receiver: Receiver<String>, client: reqwest::Client) {
    let mut queue: VecDeque<String> = VecDeque::new();
    let mut fetches = tokio::task::JoinSet::new();
    let mut open = true;
    loop {
        while fetches.len() < MAX_IN_FLIGHT
            && let Some(url) = queue.pop_back()
        {
            let Some(remote) = remote.upgrade() else {
                return;
            };
            if remote.still_wanted(&url) {
                let client = client.clone();
                fetches.spawn(async move {
                    let bytes = download(&client, &FORM_IMAGES, &url, false).await;
                    (url, bytes)
                });
            }
        }
        if !open && fetches.is_empty() && queue.is_empty() {
            return;
        }
        tokio::select! {
            url = receiver.recv(), if open => match url {
                Some(url) => {
                    queue.push_back(url);
                    if queue.len() > MAX_QUEUED
                        && let Some(oldest) = queue.pop_front()
                        && let Some(remote) = remote.upgrade()
                    {
                        remote.dropped(&oldest);
                    }
                }
                None => open = false,
            },
            Some(Ok((url, bytes))) = fetches.join_next(), if !fetches.is_empty() => {
                let Some(remote) = remote.upgrade() else {
                    return;
                };
                remote.finish(url, bytes);
            }
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
pub mod tests {
    use super::*;
    use std::io::{Read, Write};

    /// Serves `body` for every request on a local port; returns its base URL.
    pub fn serve(body: Vec<u8>) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut stream = stream;
                let mut request = [0; 1024];
                let _ = stream.read(&mut request);
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
            }
        });
        format!("http://{address}")
    }

    pub fn settle(images: &RemoteImages, url: &str) -> RemoteState {
        for _ in 0..250 {
            let state = images.state(url);
            if state != RemoteState::Loading {
                return state;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        images.state(url)
    }

    // A slow host does not hold up other images, and an image no screen still
    // asks for is never fetched.
    #[test]
    fn a_slow_image_does_not_block_the_rest() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let slow = format!("http://{}/slow.png", listener.local_addr().unwrap());
        // Accepts and never answers, like a stalled host.
        std::thread::spawn(move || {
            let held: Vec<_> = listener.incoming().flatten().collect();
            drop(held);
        });
        let images = RemoteImages::default();
        assert_eq!(images.state(&slow), RemoteState::Loading);
        let base = serve(b"fast".to_vec());
        let fast = format!("{base}/fast.png");
        let started = Instant::now();
        assert_eq!(
            settle(&images, &fast),
            RemoteState::Ready(b"fast".as_slice().into())
        );
        assert!(started.elapsed() < FORM_IMAGES.timeout);
        assert_eq!(images.state(&slow), RemoteState::Loading);

        let remote = &images.0;
        remote.entries.lock().unwrap().asked.insert(
            "http://unasked.invalid/x.png".to_owned(),
            Instant::now() - OBSOLETE_AFTER,
        );
        assert!(!remote.still_wanted("http://unasked.invalid/x.png"));
    }

    // A URL downloads once in the background; other schemes never fetch.
    #[test]
    fn remote_images_download_in_the_background() {
        let base = serve(b"image bytes".to_vec());
        let images = RemoteImages::default();
        let url = format!("{base}/button.png");
        assert_eq!(
            settle(&images, &url),
            RemoteState::Ready(b"image bytes".as_slice().into())
        );
        assert_eq!(images.state("file:///etc/passwd"), RemoteState::Failed);
    }
    #[test]
    fn review_download_completion_restores_the_cache_capacity() {
        let remote = Remote::default();
        let keys = (0..MAX_ENTRIES + 1)
            .map(|index| format!("fixture:{index}"))
            .collect::<Vec<_>>();
        {
            let mut entries = remote.entries.lock().unwrap();
            for key in &keys {
                entries.states.insert(key.clone(), RemoteState::Loading);
                entries.order.push_back(key.clone());
                entries.asked.insert(key.clone(), Instant::now());
            }
        }
        for key in keys {
            remote.finish(key, Some(vec![1]));
        }
        let entries = remote.entries.lock().unwrap();
        assert!(entries.states.len() <= MAX_ENTRIES);
        assert_eq!(entries.order.len(), entries.states.len());
        assert!(entries.asked.is_empty());
    }

    // Loading entries and queued URLs were unbounded, so a flood of new URLs grew without limit.
    #[test]
    fn loading_entries_stay_bounded_and_forgotten_urls_requeue() {
        let mut entries = Entries::default();
        for index in 0..MAX_ENTRIES + 8 {
            let key = format!("https://example.test/{index}.png");
            entries.states.insert(key.clone(), RemoteState::Loading);
            entries.order.push_back(key.clone());
            entries.asked.insert(key, Instant::now());
            entries.trim();
        }
        assert_eq!(entries.states.len(), MAX_ENTRIES);
        assert_eq!(entries.order.len(), MAX_ENTRIES);
        assert!(!entries.states.contains_key("https://example.test/0.png"));
        let newest = format!("https://example.test/{}.png", MAX_ENTRIES + 7);
        entries.forget(&newest);
        assert!(!entries.states.contains_key(&newest) && !entries.asked.contains_key(&newest));
        assert_eq!(entries.order.len(), entries.states.len());
    }
}
