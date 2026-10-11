//! Fills the core's artwork URLs with cached local files, so the menu only ever draws files on
//! disk. Polled feeds publish at once and are woken again when missing art arrives.

use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use crate::remote_images::{ImageDirectory, LAUNCHER_ART};
use crossbeam_channel::Sender;

use super::ARTWORK_BUDGET;
use bridge::{Artwork, FeaturedServer, Home, Person, Profile};
use launcher::menu::profile_achievements::visible_achievements;

/// Friends' gamerpics live apart so a long friends list cannot evict feed art.
const PEOPLE_DIR: &str = "people";

/// The feed artwork cache for one worker's runtime.
pub(super) fn feed_images(directory: &Path) -> ImageDirectory {
    ImageDirectory::new(directory.to_path_buf(), LAUNCHER_ART)
}

/// The friends' gamerpic cache for one worker's runtime.
pub(super) fn people_images(directory: &Path) -> ImageDirectory {
    ImageDirectory::new(directory.join(PEOPLE_DIR), LAUNCHER_ART)
}

/// An artwork's URL and the path to fill.
type Slot<'a> = (&'a str, &'a mut String);

fn slot(art: &mut Artwork) -> Slot<'_> {
    (&art.url, &mut art.path)
}

/// Downloads each slot's URL that has no path yet within [`ARTWORK_BUDGET`]; a failed image keeps
/// its empty path.
pub(super) async fn fill(images: &ImageDirectory, slots: Vec<Slot<'_>>) {
    let (urls, paths): (Vec<String>, Vec<&mut String>) = slots
        .into_iter()
        .filter(|(url, path)| path.is_empty() && !url.is_empty())
        .map(|(url, path)| (url.to_owned(), path))
        .unzip();
    for (path, cached) in paths
        .into_iter()
        .zip(images.fetch_all(urls, ARTWORK_BUDGET).await)
    {
        if let Some(cached) = cached {
            *path = cached.to_string_lossy().into_owned();
        }
    }
}

/// Download batches a feed may queue while its art worker is busy; later ones are dropped.
const QUEUED_BATCHES: usize = 4;
/// How long a downloaded URL is not queued again, so art beyond the cache's capacity cannot
/// evict and re-download itself in a loop.
const REFETCH_AFTER: Duration = Duration::from_secs(5 * 60);
/// Downloaded URLs a feed remembers; expired ones are dropped first.
const MAX_RECENT: usize = 1024;

/// One polled feed's art worker: a round fills what is on disk and queues the rest, and the
/// feed is woken to publish again once any of it arrives.
pub(super) struct FeedArt {
    images: ImageDirectory,
    batches: Option<Sender<Vec<String>>>,
    /// When this feed last downloaded each URL.
    recent: Arc<Mutex<HashMap<String, Instant>>>,
    /// Dropping it cancels the worker's current batch and the ones still queued.
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl FeedArt {
    /// Starts the worker; dropping this stops it without finishing queued downloads.
    pub(super) fn start(images: ImageDirectory, wake: Sender<()>) -> Self {
        let (batches, queue) = crossbeam_channel::bounded::<Vec<String>>(QUEUED_BATCHES);
        let (stop, mut stopped) = tokio::sync::oneshot::channel::<()>();
        let downloads = images.clone();
        let recent = Arc::new(Mutex::new(HashMap::new()));
        let fetched_at = Arc::clone(&recent);
        let spawned = std::thread::Builder::new()
            .name("feed-art".to_owned())
            .spawn(move || {
                let Some(runtime) = super::runtime() else {
                    return;
                };
                for urls in queue {
                    if stopped.try_recv() != Err(tokio::sync::oneshot::error::TryRecvError::Empty) {
                        return;
                    }
                    let fetched = runtime.block_on(async {
                        tokio::select! {
                            _ = &mut stopped => None,
                            fetched = downloads.fetch_all(urls.clone(), ARTWORK_BUDGET) => Some(fetched),
                        }
                    });
                    let Some(fetched) = fetched else {
                        return;
                    };
                    let arrived: Vec<String> = urls
                        .into_iter()
                        .zip(&fetched)
                        .filter_map(|(url, path)| path.is_some().then_some(url))
                        .collect();
                    // Waking only for new files keeps a dead host from looping the feed.
                    if !arrived.is_empty() {
                        remember(&fetched_at, arrived);
                        let _ = wake.try_send(());
                    }
                }
            });
        let worker = spawned
            .inspect_err(|error| tracing::warn!("feed artwork worker unavailable: {error}"))
            .ok();
        Self {
            images,
            batches: Some(batches),
            recent,
            stop: Some(stop),
            worker,
        }
    }

    /// Fills slots already cached and queues the others' downloads; never waits on the network.
    pub(super) fn fill_cached(&self, slots: Vec<Slot<'_>>) {
        let mut missing = Vec::new();
        for (url, path) in slots {
            if !path.is_empty() || url.is_empty() {
                continue;
            }
            match self.images.cached_path(url) {
                Some(cached) => *path = cached.to_string_lossy().into_owned(),
                None if !self.images.failed_recently(url) && !self.fetched_recently(url) => {
                    missing.push(url.to_owned())
                }
                None => {}
            }
        }
        if !missing.is_empty()
            && let Some(batches) = &self.batches
        {
            let _ = batches.try_send(missing);
        }
    }

    /// Whether this feed downloaded `url` within [`REFETCH_AFTER`]; gone from disk since, it was
    /// evicted for capacity.
    fn fetched_recently(&self, url: &str) -> bool {
        self.recent
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .get(url)
            .is_some_and(|at| at.elapsed() < REFETCH_AFTER)
    }
}

impl Drop for FeedArt {
    /// Stops the worker and waits for it, so a retired feed never competes for download slots.
    fn drop(&mut self) {
        self.stop.take();
        self.batches.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Records `urls` as just downloaded, keeping at most [`MAX_RECENT`] entries.
fn remember(recent: &Mutex<HashMap<String, Instant>>, urls: Vec<String>) {
    let mut recent = recent.lock().unwrap_or_else(|poison| poison.into_inner());
    if recent.len() + urls.len() > MAX_RECENT {
        recent.retain(|_, at| at.elapsed() < REFETCH_AFTER);
    }
    if recent.len() + urls.len() > MAX_RECENT {
        recent.clear();
    }
    let now = Instant::now();
    recent.extend(urls.into_iter().map(|url| (url, now)));
}

/// Logos, banners, screenshots and activity art of the featured servers.
pub(super) fn featured_slots(servers: &mut [FeaturedServer]) -> Vec<Slot<'_>> {
    servers
        .iter_mut()
        .flat_map(|server| {
            let FeaturedServer {
                logo,
                background,
                screenshots,
                games,
                ..
            } = server;
            [logo, background]
                .into_iter()
                .chain(screenshots.iter_mut())
                .chain(games.iter_mut().map(|game| &mut game.image))
                .map(slot)
        })
        .collect()
}

/// Messaging images and the persona head.
pub(super) fn home_slots(home: &mut Home) -> Vec<Slot<'_>> {
    let Home {
        messages,
        persona_head,
        ..
    } = home;
    messages
        .iter_mut()
        .flat_map(|message| message.images.iter_mut())
        .map(|image| (image.url.as_str(), &mut image.path))
        .chain(std::iter::once(slot(persona_head)))
        .collect()
}

/// The gamerpic, featured screenshot and the achievements Overview shows.
pub(super) fn profile_slots(profile: &mut Profile) -> Vec<Slot<'_>> {
    let shown: Vec<usize> = profile
        .achievements
        .as_ref()
        .map(|summary| {
            let entries = &summary.entries;
            visible_achievements(entries)
                .into_iter()
                .flatten()
                .filter_map(|chosen| entries.iter().position(|entry| std::ptr::eq(entry, chosen)))
                .collect()
        })
        .unwrap_or_default();
    let Profile {
        gamerpic,
        featured_screenshot,
        achievements,
        ..
    } = profile;
    let overview = achievements
        .iter_mut()
        .flat_map(|summary| summary.entries.iter_mut().enumerate())
        .filter(|(index, _)| shown.contains(index))
        .map(|(_, entry)| slot(&mut entry.image));
    [slot(gamerpic), slot(featured_screenshot)]
        .into_iter()
        .chain(overview)
        .collect()
}

/// Every friend's gamerpic.
pub(super) fn people_slots(people: &mut [Person]) -> Vec<Slot<'_>> {
    people
        .iter_mut()
        .map(|person| slot(&mut person.gamerpic))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn art(url: &str, path: &str) -> Artwork {
        Artwork {
            url: url.to_owned(),
            path: path.to_owned(),
        }
    }

    // The core no longer caches artwork, so every URL the menu draws must reach a slot.
    #[test]
    fn every_drawn_artwork_url_reaches_a_slot() {
        let mut servers = vec![FeaturedServer {
            logo: art("https://a.test/logo", ""),
            background: art("https://a.test/bg", ""),
            screenshots: vec![art("https://a.test/shot", "")],
            games: vec![bridge::FeaturedGame {
                image: art("https://a.test/game", ""),
                ..Default::default()
            }],
            ..Default::default()
        }];
        let urls: Vec<_> = featured_slots(&mut servers)
            .into_iter()
            .map(|(url, _)| url.to_owned())
            .collect();
        assert_eq!(
            urls,
            ["logo", "bg", "shot", "game"].map(|name| format!("https://a.test/{name}"))
        );
        let mut people = vec![Person {
            gamerpic: art("https://a.test/pic", ""),
            ..Default::default()
        }];
        assert_eq!(people_slots(&mut people)[0].0, "https://a.test/pic");
    }

    #[test]
    fn profile_slots_cover_only_overview_achievements() {
        let entry = |id: &str, locked: bool, order: Option<u32>| bridge::ProfileAchievement {
            id: id.to_owned(),
            image: art(&format!("https://a.test/{id}"), ""),
            locked,
            suggested_order: order,
            ..Default::default()
        };
        let mut profile = Profile {
            gamerpic: art("https://a.test/pic", ""),
            avatar: art("", "/core/persona-avatar.img"),
            achievements: Some(bridge::ProfileAchievements {
                entries: vec![entry("next", true, Some(1)), entry("hidden", true, None)],
                ..Default::default()
            }),
            ..Default::default()
        };
        let urls: Vec<_> = profile_slots(&mut profile)
            .into_iter()
            .map(|(url, _)| url.to_owned())
            .collect();
        assert!(urls.contains(&"https://a.test/next".to_owned()));
        assert!(!urls.contains(&"https://a.test/hidden".to_owned()));
        assert!(urls.contains(&"https://a.test/pic".to_owned()));
    }

    #[test]
    fn filled_paths_are_kept_and_failed_images_stay_empty() {
        let dir = tempfile::tempdir().unwrap();
        let images = feed_images(dir.path());
        let mut home = Home {
            persona_head: art("", "/core/persona-head.img"),
            messages: vec![bridge::Message {
                images: vec![bridge::MessageImage {
                    id: "tile".to_owned(),
                    url: "http://insecure.test/t.png".to_owned(),
                    path: String::new(),
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(fill(&images, home_slots(&mut home)));
        assert_eq!(home.persona_head.path, "/core/persona-head.img");
        assert!(home.messages[0].images[0].path.is_empty());
    }

    // Feeds waited for every image download, so one slow host held back each publish.
    #[test]
    fn feed_rounds_publish_before_their_art_downloads() {
        use std::io::{Read, Write};
        use std::time::{Duration, Instant};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/tile.png", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let _ = stream.read(&mut [0; 1024]);
                std::thread::sleep(Duration::from_millis(300));
                let body = b"\x89PNG\r\n\x1a\nrest";
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(body);
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let local = crate::remote_images::Surface {
            https_only: false,
            public_hosts: false,
            ..LAUNCHER_ART
        };
        let (wake, woken) = crossbeam_channel::bounded(1);
        let feed = FeedArt::start(ImageDirectory::new(dir.path().to_path_buf(), local), wake);
        let mut home = Home {
            persona_head: art(&url, ""),
            ..Default::default()
        };
        let started = Instant::now();
        feed.fill_cached(home_slots(&mut home));
        assert!(started.elapsed() < Duration::from_millis(250));
        assert!(home.persona_head.path.is_empty());
        woken.recv_timeout(Duration::from_secs(5)).unwrap();
        feed.fill_cached(home_slots(&mut home));
        assert!(!home.persona_head.path.is_empty());
    }

    // Art beyond the cache's capacity evicted itself and was queued again on every wake.
    #[test]
    fn evicted_art_is_not_queued_again_right_after_downloading() {
        let dir = tempfile::tempdir().unwrap();
        let (batches, queued) = crossbeam_channel::bounded(QUEUED_BATCHES);
        let feed = FeedArt {
            images: feed_images(dir.path()),
            batches: Some(batches),
            recent: Arc::default(),
            stop: None,
            worker: None,
        };
        let url = "https://a.test/evicted.png";
        let mut home = Home {
            persona_head: art(url, ""),
            ..Default::default()
        };
        feed.fill_cached(home_slots(&mut home));
        assert_eq!(queued.try_recv().unwrap(), [url]);
        remember(&feed.recent, vec![url.to_owned()]);
        feed.fill_cached(home_slots(&mut home));
        assert!(queued.try_recv().is_err(), "evicted art queued again");
    }

    #[test]
    fn remembered_downloads_are_bounded() {
        let recent = Mutex::new(HashMap::new());
        for batch in 0..3 {
            let urls = (0..MAX_RECENT / 2)
                .map(|index| format!("https://a.test/{batch}/{index}"))
                .collect();
            remember(&recent, urls);
        }
        assert!(recent.lock().unwrap().len() <= MAX_RECENT);
    }

    // A retired feed's worker kept its stalled download open, holding the folder's slot.
    #[test]
    fn dropping_a_feed_closes_its_downloads() {
        use std::io::Read;
        use std::time::Duration;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/stalled.png", listener.local_addr().unwrap());
        // Accepts and never answers, like a stalled host, handing each connection to the test.
        let (accepted, connections) = crossbeam_channel::unbounded();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                if accepted.send(stream).is_err() {
                    return;
                }
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let local = crate::remote_images::Surface {
            https_only: false,
            public_hosts: false,
            ..LAUNCHER_ART
        };
        let (wake, _woken) = crossbeam_channel::bounded(1);
        let feed = FeedArt::start(ImageDirectory::new(dir.path().to_path_buf(), local), wake);
        let mut home = Home {
            persona_head: art(&url, ""),
            ..Default::default()
        };
        feed.fill_cached(home_slots(&mut home));
        let mut stream = connections
            .recv_timeout(Duration::from_secs(5))
            .expect("the download never connected");
        drop(feed);
        // Shorter than the surface timeout, so only a cancelled download closes in time.
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut request = Vec::new();
        assert!(
            stream.read_to_end(&mut request).is_ok(),
            "the retired feed kept its download open"
        );
    }
}
