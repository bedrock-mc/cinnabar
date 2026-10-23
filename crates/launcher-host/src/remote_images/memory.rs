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
/// Retained download bytes; handed-off bodies are released first. Bodies the engine has not
/// received may exceed this budget until their first handoff.
const MAX_READY_BYTES: usize = FORM_IMAGES.max_bytes;
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
    /// The engine received this image, but its download body is no longer retained.
    ReadyWithoutBody,
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
    states: HashMap<String, Entry>,
    order: VecDeque<String>,
    /// When a drawn screen last asked for each loading URL.
    asked: HashMap<String, Instant>,
    /// Total length of the `Ready` images in `states`.
    ready_bytes: usize,
}

struct Entry {
    state: RemoteState,
    handed: bool,
}

impl Entry {
    /// Remembers a state whose body has not been handed to the engine yet.
    fn new(state: RemoteState) -> Self {
        Self {
            state,
            handed: false,
        }
    }
}

impl Entries {
    /// Bounds remembered URLs and releases handed-off bodies without changing readiness.
    /// Unclaimed bodies survive either limit until the engine receives them.
    fn trim(&mut self) {
        while self.order.len() > MAX_ENTRIES {
            let eligible = |key: &String| {
                self.states.get(key).is_some_and(|entry| {
                    entry.handed || !matches!(entry.state, RemoteState::Ready(_))
                })
            };
            let Some(position) = self
                .order
                .iter()
                .position(|key| eligible(key) && self.states[key].state != RemoteState::Loading)
                .or_else(|| self.order.iter().position(eligible))
            else {
                break;
            };
            if let Some(key) = self.order.remove(position) {
                if let Some(Entry {
                    state: RemoteState::Ready(bytes),
                    ..
                }) = self.states.remove(&key)
                {
                    self.ready_bytes -= bytes.len();
                }
                self.asked.remove(&key);
            }
        }
        for key in &self.order {
            if self.ready_bytes <= MAX_READY_BYTES {
                break;
            }
            let Some(entry) = self.states.get_mut(key) else {
                continue;
            };
            if entry.handed
                && let RemoteState::Ready(bytes) = &entry.state
            {
                self.ready_bytes -= bytes.len();
                entry.state = RemoteState::ReadyWithoutBody;
            }
        }
    }

    /// Forgets a loading URL so a later ask queues it afresh.
    fn forget(&mut self, url: &str) {
        self.asked.remove(url);
        if self
            .states
            .get(url)
            .is_some_and(|entry| entry.state == RemoteState::Loading)
        {
            self.states.remove(url);
            self.order.retain(|key| key != url);
        }
    }
}

impl RemoteImages {
    /// The download state of `url`, starting its download on first sight.
    pub fn state(&self, url: &str) -> RemoteState {
        self.lookup(url, false)
    }

    /// Requests bytes for an atlas miss, refetching a released body once if needed.
    pub fn load(&self, url: &str) -> RemoteState {
        self.lookup(url, true)
    }

    /// Polls display readiness or hands a retained body to the engine.
    fn lookup(&self, url: &str, needs_body: bool) -> RemoteState {
        let mut entries = self.0.entries.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(entry) = entries.states.get_mut(url)
            && !(needs_body && entry.state == RemoteState::ReadyWithoutBody)
        {
            let state = entry.state.clone();
            if needs_body && matches!(state, RemoteState::Ready(_)) {
                entry.handed = true;
                entries.trim();
            }
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
        if entries
            .states
            .insert(url.to_owned(), Entry::new(state.clone()))
            .is_none()
        {
            entries.order.push_back(url.to_owned());
        }
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
        if let Some(entry) = entries.states.get_mut(&url) {
            let replaced = match &entry.state {
                RemoteState::Ready(old) => old.len(),
                _ => 0,
            };
            entry.state = bytes.map_or(RemoteState::Failed, |bytes| {
                RemoteState::Ready(bytes.into())
            });
            entry.handed = false;
            let added = match &entry.state {
                RemoteState::Ready(new) => new.len(),
                _ => 0,
            };
            entries.ready_bytes = entries.ready_bytes - replaced + added;
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
    #[cfg(test)]
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

    /// Waits for the image fixture to finish its request.
    #[cfg(test)]
    fn settle(images: &RemoteImages, url: &str) -> RemoteState {
        test_time::wait_until(Duration::from_secs(5), || {
            images.state(url) != RemoteState::Loading
        });
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
    fn form_url_images_larger_than_two_megabytes_are_downloaded() {
        let body = vec![7; 2 * 1024 * 1024 + 1];
        let url = format!("{}/large.png?size=1024", serve(body.clone()));
        let RemoteState::Ready(downloaded) = settle(&RemoteImages::default(), &url) else {
            panic!("image download failed");
        };
        assert_eq!(downloaded.len(), body.len());
        assert!(downloaded.iter().all(|byte| *byte == 7));
    }

    #[test]
    fn two_forty_megabyte_images_stay_ready_without_repeated_fetches() {
        let images = RemoteImages::default();
        let (sender, mut requests) = tokio::sync::mpsc::channel(MAX_QUEUED);
        *images.0.worker.lock().unwrap() = Some(sender);
        let urls = [
            "http://example.test/first.png",
            "http://example.test/second.png",
        ];
        for url in urls {
            assert_eq!(images.state(url), RemoteState::Loading);
            assert_eq!(requests.try_recv().unwrap(), url);
            images
                .0
                .finish(url.to_owned(), Some(vec![0; 40 * 1024 * 1024]));
            assert!(matches!(images.load(url), RemoteState::Ready(_)));
        }
        for _ in 0..3 {
            for url in urls {
                assert!(matches!(
                    images.state(url),
                    RemoteState::Ready(_) | RemoteState::ReadyWithoutBody
                ));
            }
            assert!(
                requests.try_recv().is_err(),
                "display polling must not refetch"
            );
        }
        assert_eq!(images.state(urls[0]), RemoteState::ReadyWithoutBody);
        assert_eq!(images.load(urls[0]), RemoteState::Loading);
        assert_eq!(requests.try_recv().unwrap(), urls[0]);
        assert_eq!(images.load(urls[0]), RemoteState::Loading);
        assert!(
            requests.try_recv().is_err(),
            "an atlas miss starts only one fetch"
        );
        images
            .0
            .finish(urls[0].to_owned(), Some(vec![0; 40 * 1024 * 1024]));
        assert!(matches!(images.load(urls[0]), RemoteState::Ready(_)));
        assert_eq!(images.state(urls[1]), RemoteState::ReadyWithoutBody);
        assert!(requests.try_recv().is_err());
    }

    #[test]
    fn review_download_completion_restores_the_cache_capacity() {
        let remote = Remote::default();
        {
            let mut entries = remote.entries.lock().unwrap();
            for index in 0..MAX_ENTRIES {
                let key = format!("fixture:{index}");
                entries
                    .states
                    .insert(key.clone(), Entry::new(RemoteState::Failed));
                entries.order.push_back(key);
            }
            let key = "fixture:newest".to_owned();
            entries
                .states
                .insert(key.clone(), Entry::new(RemoteState::Loading));
            entries.order.push_back(key.clone());
            entries.asked.insert(key, Instant::now());
        }
        remote.finish("fixture:newest".to_owned(), Some(vec![1]));
        let entries = remote.entries.lock().unwrap();
        assert_eq!(entries.states.len(), MAX_ENTRIES);
        assert_eq!(entries.order.len(), entries.states.len());
        assert!(entries.asked.is_empty());
        assert!(matches!(
            entries.states["fixture:newest"].state,
            RemoteState::Ready(_)
        ));
    }

    #[test]
    fn unclaimed_images_survive_the_byte_budget_until_handoff() {
        let images = RemoteImages::default();
        let urls = ["fixture:first", "fixture:second"];
        for url in urls {
            let mut entries = images.0.entries.lock().unwrap();
            entries
                .states
                .insert(url.to_owned(), Entry::new(RemoteState::Loading));
            entries.order.push_back(url.to_owned());
        }
        for url in urls {
            images
                .0
                .finish(url.to_owned(), Some(vec![0; MAX_READY_BYTES + 1]));
        }
        for url in urls {
            let RemoteState::Ready(bytes) = images.load(url) else {
                panic!("an unclaimed image must reach the engine even beyond the budget");
            };
            assert_eq!(bytes.len(), MAX_READY_BYTES + 1);
            assert_eq!(images.state(url), RemoteState::ReadyWithoutBody);
        }
        assert_eq!(images.0.entries.lock().unwrap().ready_bytes, 0);
    }

    // Large form images counted only toward the entry limit, so 64 of them could keep GiBs resident.
    #[test]
    fn downloaded_images_stay_within_the_byte_budget() {
        let images = RemoteImages::default();
        let remote = &images.0;
        let size = MAX_READY_BYTES / 4 + 1;
        let keys = (0..5)
            .map(|index| format!("fixture:{index}"))
            .collect::<Vec<_>>();
        for key in &keys {
            {
                let mut entries = remote.entries.lock().unwrap();
                entries
                    .states
                    .insert(key.clone(), Entry::new(RemoteState::Loading));
                entries.order.push_back(key.clone());
            }
            remote.finish(key.clone(), Some(vec![0; size]));
            assert!(matches!(images.load(key), RemoteState::Ready(_)));
        }
        let entries = remote.entries.lock().unwrap();
        assert!(entries.ready_bytes <= MAX_READY_BYTES);
        assert_eq!(entries.ready_bytes, 3 * size);
        assert_eq!(
            entries.states["fixture:0"].state,
            RemoteState::ReadyWithoutBody
        );
        assert_eq!(
            entries.states["fixture:1"].state,
            RemoteState::ReadyWithoutBody
        );
        assert!(entries.states.contains_key("fixture:4"));
        assert_eq!(entries.order.len(), entries.states.len());
    }

    #[test]
    fn byte_evicted_images_download_again_only_for_an_atlas_miss() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/button.png", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            for body in [b"first".as_slice(), b"replacement".as_slice()] {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0; 1024];
                assert!(stream.read(&mut request).unwrap() > 0);
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                stream.write_all(body).unwrap();
            }
        });
        let images = RemoteImages::default();
        assert_eq!(
            settle(&images, &url),
            RemoteState::Ready(b"first".as_slice().into())
        );
        assert!(matches!(images.load(&url), RemoteState::Ready(_)));
        let key = "fixture:full-budget".to_owned();
        {
            let mut entries = images.0.entries.lock().unwrap();
            entries
                .states
                .insert(key.clone(), Entry::new(RemoteState::Loading));
            entries.order.push_back(key.clone());
        }
        images.0.finish(key.clone(), Some(vec![0; MAX_READY_BYTES]));
        {
            let entries = images.0.entries.lock().unwrap();
            assert_eq!(entries.states[&url].state, RemoteState::ReadyWithoutBody);
            assert!(matches!(entries.states[&key].state, RemoteState::Ready(_)));
            assert_eq!(entries.ready_bytes, MAX_READY_BYTES);
        }
        assert_eq!(images.state(&url), RemoteState::ReadyWithoutBody);
        assert!(matches!(images.load(&key), RemoteState::Ready(_)));
        assert_eq!(images.load(&url), RemoteState::Loading);
        assert_eq!(
            settle(&images, &url),
            RemoteState::Ready(b"replacement".as_slice().into())
        );
        server.join().unwrap();
        let entries = images.0.entries.lock().unwrap();
        assert_eq!(entries.states[&key].state, RemoteState::ReadyWithoutBody);
        assert_eq!(entries.ready_bytes, b"replacement".len());
    }

    // Loading entries and queued URLs were unbounded, so a flood of new URLs grew without limit.
    #[test]
    fn loading_entries_stay_bounded_and_forgotten_urls_requeue() {
        let mut entries = Entries::default();
        for index in 0..MAX_ENTRIES + 8 {
            let key = format!("https://example.test/{index}.png");
            entries
                .states
                .insert(key.clone(), Entry::new(RemoteState::Loading));
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
