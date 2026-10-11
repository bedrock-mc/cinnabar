use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::atomic::AtomicUsize,
    time::Duration,
};

use super::super::{FORM_IMAGES, LAUNCHER_ART, STORE_ART, client_with};
use super::*;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nrest";

/// A loopback surface: the launcher policy without the public-address rule.
fn local(surface: Surface) -> Surface {
    Surface {
        https_only: false,
        public_hosts: false,
        ..surface
    }
}

/// Serves `respond(path)` as `(status line, extra headers, body)`; counts requests and the
/// most connections open at once.
struct Server {
    base: String,
    requests: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
}

fn serve(
    delay: Duration,
    respond: impl Fn(&str) -> (&'static str, String, Vec<u8>) + Send + Sync + 'static,
) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (requests, peak, open) = (
        Arc::new(AtomicUsize::new(0)),
        Arc::new(AtomicUsize::new(0)),
        Arc::new(AtomicUsize::new(0)),
    );
    let respond = Arc::new(respond);
    let (counted, highest) = (Arc::clone(&requests), Arc::clone(&peak));
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let (respond, counted, highest, open) = (
                Arc::clone(&respond),
                Arc::clone(&counted),
                Arc::clone(&highest),
                Arc::clone(&open),
            );
            std::thread::spawn(move || {
                let now = open.fetch_add(1, Ordering::SeqCst) + 1;
                highest.fetch_max(now, Ordering::SeqCst);
                counted.fetch_add(1, Ordering::SeqCst);
                let mut request = [0; 2048];
                let read = stream.read(&mut request).unwrap_or(0);
                let text = String::from_utf8_lossy(&request[..read]);
                let path = text.split(' ').nth(1).unwrap_or("/").to_owned();
                std::thread::sleep(delay);
                let (status, headers, body) = respond(&path);
                let head = format!(
                    "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
                open.fetch_sub(1, Ordering::SeqCst);
            });
        }
    });
    Server {
        base,
        requests,
        peak,
    }
}

fn image_server() -> Server {
    serve(Duration::ZERO, |_| ("200 OK", String::new(), PNG.to_vec()))
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

#[test]
fn a_download_is_cached_and_reused_without_refetching() {
    let dir = tempfile::tempdir().unwrap();
    let server = image_server();
    let cache = ImageDirectory::new(dir.path().to_path_buf(), local(LAUNCHER_ART));
    let url = format!("{}/logo.png", server.base);
    let runtime = runtime();
    let first = runtime.block_on(cache.fetch(&url)).unwrap();
    assert_eq!(first.extension().unwrap(), "img");
    assert_eq!(fs::read(&first).unwrap(), PNG);
    let digest = hex(&Sha256::digest(url.as_bytes()));
    assert_eq!(first, dir.path().join(format!("{digest}.img")));
    assert_eq!(runtime.block_on(cache.fetch(&url)), Some(first));
    assert_eq!(server.requests.load(Ordering::SeqCst), 1);
}

#[test]
fn signature_named_files_take_their_image_extension() {
    let dir = tempfile::tempdir().unwrap();
    let server = serve(Duration::ZERO, |_| {
        ("200 OK", String::new(), b"GIF89a...".to_vec())
    });
    let cache = ImageDirectory::new(dir.path().to_path_buf(), local(STORE_ART));
    let path = runtime()
        .block_on(cache.fetch(&format!("{}/a", server.base)))
        .unwrap();
    assert_eq!(path.extension().unwrap(), "gif");
}

#[test]
fn refused_payloads_never_publish_files() {
    let dir = tempfile::tempdir().unwrap();
    let server = serve(Duration::ZERO, |path| match path {
        "/text" => ("200 OK", String::new(), b"<html>".to_vec()),
        "/huge" => ("200 OK", String::new(), [PNG, &[0u8; 64][..]].concat()),
        "/partial" => ("206 Partial Content", String::new(), PNG.to_vec()),
        _ => ("404 Not Found", String::new(), Vec::new()),
    });
    let surface = Surface {
        max_bytes: 32,
        ..local(STORE_ART)
    };
    let cache = ImageDirectory::new(dir.path().to_path_buf(), surface);
    let runtime = runtime();
    for path in ["/text", "/huge", "/partial", "/missing"] {
        let url = format!("{}{path}", server.base);
        assert_eq!(runtime.block_on(cache.fetch(&url)), None, "{path} cached");
    }
    let long = format!("{}/{}", server.base, "a".repeat(STORE_ART.max_url_bytes));
    assert_eq!(runtime.block_on(cache.fetch(&long)), None);
    let left: Vec<_> = fs::read_dir(dir.path()).into_iter().flatten().collect();
    assert!(left.is_empty(), "left {left:?}");
}

#[test]
fn redirects_beyond_the_surface_bound_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let server = serve(Duration::ZERO, |path| {
        let hop: usize = path.trim_start_matches("/hop").parse().unwrap_or(0);
        if hop < 4 {
            let next = format!("Location: /hop{}\r\n", hop + 1);
            ("302 Found", next, Vec::new())
        } else {
            ("200 OK", String::new(), PNG.to_vec())
        }
    });
    let runtime = runtime();
    let within = Surface {
        max_redirects: 4,
        ..local(STORE_ART)
    };
    let cache = ImageDirectory::new(dir.path().join("within"), within);
    assert!(
        runtime
            .block_on(cache.fetch(&format!("{}/hop0", server.base)))
            .is_some()
    );
    let beyond = Surface {
        max_redirects: 3,
        ..local(STORE_ART)
    };
    let cache = ImageDirectory::new(dir.path().join("beyond"), beyond);
    assert!(
        runtime
            .block_on(cache.fetch(&format!("{}/hop0", server.base)))
            .is_none()
    );
}

#[test]
fn eviction_keeps_the_most_recently_used_files() {
    let dir = tempfile::tempdir().unwrap();
    let server = image_server();
    let surface = Surface {
        max_files: 2,
        ..local(LAUNCHER_ART)
    };
    let cache = ImageDirectory::new(dir.path().to_path_buf(), surface);
    let runtime = runtime();
    let url = |name: &str| format!("{}/{name}", server.base);
    let first = runtime.block_on(cache.fetch(&url("a"))).unwrap();
    let old = SystemTime::now() - Duration::from_secs(60);
    fs::File::options()
        .write(true)
        .open(&first)
        .unwrap()
        .set_modified(old)
        .unwrap();
    let second = runtime.block_on(cache.fetch(&url("b"))).unwrap();
    // A hit refreshes the first file, so the second becomes the oldest.
    fs::File::options()
        .write(true)
        .open(&second)
        .unwrap()
        .set_modified(old - Duration::from_secs(60))
        .unwrap();
    assert_eq!(
        runtime.block_on(cache.fetch(&url("a"))),
        Some(first.clone())
    );
    let third = runtime.block_on(cache.fetch(&url("c"))).unwrap();
    assert!(first.exists() && third.exists());
    assert!(!second.exists());
}

// The core's persona art shares the launcher folder; as the oldest files there it went first.
#[test]
fn eviction_leaves_files_this_cache_did_not_name() {
    let dir = tempfile::tempdir().unwrap();
    let old = SystemTime::now() - Duration::from_secs(3_600);
    for name in ["persona-head.img", "persona-avatar-ab.img", "notes.txt"] {
        let path = dir.path().join(name);
        fs::write(&path, PNG).unwrap();
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(old)
            .unwrap();
    }
    let server = image_server();
    let surface = Surface {
        max_files: 1,
        ..local(LAUNCHER_ART)
    };
    let cache = ImageDirectory::new(dir.path().to_path_buf(), surface);
    let runtime = runtime();
    let first = runtime
        .block_on(cache.fetch(&format!("{}/a", server.base)))
        .unwrap();
    assert!(first.exists(), "foreign files counted toward the bound");
    // Keep eviction order independent of the filesystem's timestamp resolution.
    fs::File::options()
        .write(true)
        .open(&first)
        .unwrap()
        .set_modified(old + Duration::from_secs(60))
        .unwrap();
    let second = runtime
        .block_on(cache.fetch(&format!("{}/b", server.base)))
        .unwrap();
    assert!(second.exists() && !first.exists());
    for name in ["persona-head.img", "persona-avatar-ab.img", "notes.txt"] {
        assert!(dir.path().join(name).exists(), "{name} evicted");
    }
    assert!(cache_file(&format!("{}.img", "a".repeat(64))));
    assert!(!cache_file(&"a".repeat(64)));
    assert!(!cache_file(&format!("{}.png", "A".repeat(64))));
}

#[test]
fn directory_bytes_are_bounded_too() {
    let dir = tempfile::tempdir().unwrap();
    let server = image_server();
    let surface = Surface {
        max_dir_bytes: PNG.len() as u64 * 2,
        ..local(STORE_ART)
    };
    let cache = ImageDirectory::new(dir.path().to_path_buf(), surface);
    let runtime = runtime();
    for name in ["a", "b", "c", "d"] {
        runtime.block_on(cache.fetch(&format!("{}/{name}", server.base)));
    }
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
}

#[test]
fn fetch_all_keeps_order_and_bounds_concurrent_downloads() {
    let dir = tempfile::tempdir().unwrap();
    let server = serve(Duration::from_millis(50), |path| {
        if path == "/bad" {
            ("200 OK", String::new(), b"nope".to_vec())
        } else {
            ("200 OK", String::new(), PNG.to_vec())
        }
    });
    let cache = ImageDirectory::new(dir.path().to_path_buf(), local(LAUNCHER_ART));
    let mut urls: Vec<String> = (0..20)
        .map(|index| format!("{}/{index}", server.base))
        .collect();
    urls[3] = format!("{}/bad", server.base);
    let results = runtime().block_on(cache.fetch_all(urls.clone(), Duration::from_secs(30)));
    assert_eq!(results.len(), urls.len());
    assert!(results[3].is_none());
    for (index, url) in urls.iter().enumerate().filter(|(index, _)| *index != 3) {
        let digest = hex(&Sha256::digest(url.as_bytes()));
        assert_eq!(
            results[index],
            Some(dir.path().join(format!("{digest}.img")))
        );
    }
    let peak = server.peak.load(Ordering::SeqCst);
    assert!((2..=MAX_IN_FLIGHT).contains(&peak), "peak {peak}");
}

// Stalled hosts held a feed past its deadline; finished images must survive the cut-off.
#[test]
fn fetch_all_abandons_downloads_past_its_budget_but_keeps_finished_ones() {
    let dir = tempfile::tempdir().unwrap();
    let server = serve(Duration::ZERO, |path| {
        if path == "/stall" {
            std::thread::sleep(Duration::from_secs(5));
        }
        ("200 OK", String::new(), PNG.to_vec())
    });
    let cache = ImageDirectory::new(dir.path().to_path_buf(), local(LAUNCHER_ART));
    let urls = vec![
        format!("{}/fast", server.base),
        format!("{}/stall", server.base),
    ];
    let started = std::time::Instant::now();
    // The runtime stays alive but idle, as a worker's does between batches.
    let runtime = runtime();
    let results = runtime.block_on(cache.fetch_all(urls, Duration::from_millis(500)));
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(results[0].is_some());
    assert!(results[1].is_none());
    assert_eq!(
        cache.0.folder.slots.available_permits(),
        MAX_IN_FLIGHT,
        "an abandoned download still holds its slot"
    );
}

// Concurrent fetches of one URL each passed the cache check and downloaded it again.
#[test]
fn a_repeated_url_in_one_batch_downloads_once() {
    let dir = tempfile::tempdir().unwrap();
    let server = serve(Duration::from_millis(100), |_| {
        ("200 OK", String::new(), PNG.to_vec())
    });
    let cache = ImageDirectory::new(dir.path().to_path_buf(), local(LAUNCHER_ART));
    let url = format!("{}/shared.png", server.base);
    let other = format!("{}/other.png", server.base);
    let urls = vec![url.clone(), other, url.clone(), url];
    let results = runtime().block_on(cache.fetch_all(urls, Duration::from_secs(30)));
    assert!(results.iter().all(Option::is_some));
    assert_eq!(results[0], results[2]);
    assert_eq!(results[0], results[3]);
    assert_ne!(results[0], results[1]);
    assert_eq!(server.requests.load(Ordering::SeqCst), 2);
}

#[test]
fn waiting_callers_are_bounded() {
    let count = AtomicUsize::new(0);
    let held: Vec<_> = (0..MAX_WAITING).map(|_| Waiting::enter(&count)).collect();
    assert!(held.iter().all(Option::is_some));
    assert!(Waiting::enter(&count).is_none());
    drop(held);
    assert_eq!(count.load(Ordering::SeqCst), 0);
    assert!(Waiting::enter(&count).is_some());
}

/// Resolves every host to loopback, as a hostile DNS answer would.
struct Loopback;

impl reqwest::dns::Resolve for Loopback {
    fn resolve(&self, _: reqwest::dns::Name) -> reqwest::dns::Resolving {
        Box::pin(async {
            let loopback = std::net::SocketAddr::from(([127, 0, 0, 1], 0));
            Ok::<reqwest::dns::Addrs, Box<dyn std::error::Error + Send + Sync>>(Box::new(
                std::iter::once(loopback),
            ))
        })
    }
}

/// Whether anything connected to `listener` so far.
fn contacted(listener: &TcpListener) -> bool {
    listener.set_nonblocking(true).unwrap();
    listener.accept().is_ok()
}

// The deleted Go dialer refused names resolving inward; the resolver must keep doing so.
#[test]
fn a_public_surface_never_connects_to_a_host_resolving_to_loopback() {
    let dir = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let cache = ImageDirectory::new(dir.path().to_path_buf(), LAUNCHER_ART);
    let url = format!("https://localhost:{port}/art.png");
    assert!(LAUNCHER_ART.accepts(&url));
    assert_eq!(runtime().block_on(cache.fetch(&url)), None);
    assert!(!contacted(&listener), "connected to a loopback host");
}

// A public first hop must not redirect a download onto a local address.
#[test]
fn a_public_surface_never_follows_a_redirect_to_a_private_address() {
    let dir = tempfile::tempdir().unwrap();
    let inner = TcpListener::bind("127.0.0.1:0").unwrap();
    let target = format!("http://{}/private.png", inner.local_addr().unwrap());
    let outer = serve(Duration::ZERO, move |_| {
        ("302 Found", format!("Location: {target}\r\n"), Vec::new())
    });
    let port = outer.base.rsplit(':').next().unwrap().to_owned();
    // Plain HTTP so the fixture needs no certificate; the address rules are the public ones.
    let surface = Surface {
        https_only: false,
        ..LAUNCHER_ART
    };
    let client = client_with(&surface, Arc::new(Loopback));
    let cache = ImageDirectory::with_client(dir.path().to_path_buf(), surface, client);
    let url = format!("http://public.example.test:{port}/art.png");
    assert_eq!(runtime().block_on(cache.fetch(&url)), None);
    assert_eq!(
        outer.requests.load(Ordering::SeqCst),
        1,
        "first hop not served"
    );
    assert!(
        !contacted(&inner),
        "followed a redirect to a private address"
    );
}

// A dead host was asked again on every feed round; failures are now remembered for a while.
#[test]
fn failed_urls_wait_before_downloading_again() {
    let dir = tempfile::tempdir().unwrap();
    let server = serve(Duration::ZERO, |_| {
        ("200 OK", String::new(), b"nope".to_vec())
    });
    let cache = ImageDirectory::new(dir.path().to_path_buf(), local(LAUNCHER_ART));
    let url = format!("{}/broken", server.base);
    let runtime = runtime();
    assert_eq!(runtime.block_on(cache.fetch(&url)), None);
    assert!(cache.failed_recently(&url));
    assert_eq!(runtime.block_on(cache.fetch(&url)), None);
    assert_eq!(server.requests.load(Ordering::SeqCst), 1);
    lock(&cache.0.folder.failed)
        .at
        .insert(url.clone(), Instant::now() - RETRY_AFTER);
    assert_eq!(runtime.block_on(cache.fetch(&url)), None);
    assert_eq!(server.requests.load(Ordering::SeqCst), 2);
}

#[test]
fn remembered_failures_are_bounded() {
    let mut failures = Failures::default();
    for index in 0..MAX_FAILED + 5 {
        failures.record(&format!("https://a.test/{index}"));
    }
    assert_eq!(failures.at.len(), MAX_FAILED);
    assert!(!failures.recent("https://a.test/0"));
    assert!(failures.recent(&format!("https://a.test/{}", MAX_FAILED + 4)));
}

// The download bound was per instance, while several workers share one folder.
#[test]
fn instances_on_one_folder_share_its_download_slots() {
    let dir = tempfile::tempdir().unwrap();
    let a = ImageDirectory::new(dir.path().join("x"), LAUNCHER_ART);
    let b = ImageDirectory::new(dir.path().join("x"), LAUNCHER_ART);
    let other = ImageDirectory::new(dir.path().join("y"), LAUNCHER_ART);
    assert!(Arc::ptr_eq(&a.0.folder, &b.0.folder));
    assert!(!Arc::ptr_eq(&a.0.folder, &other.0.folder));
}

#[test]
fn cached_paths_never_touch_the_network() {
    let dir = tempfile::tempdir().unwrap();
    let server = image_server();
    let cache = ImageDirectory::new(dir.path().to_path_buf(), local(LAUNCHER_ART));
    let url = format!("{}/logo", server.base);
    assert_eq!(cache.cached_path(&url), None);
    let fetched = runtime().block_on(cache.fetch(&url));
    assert_eq!(cache.cached_path(&url), fetched);
    assert_eq!(server.requests.load(Ordering::SeqCst), 1);
}

#[test]
fn public_surfaces_refuse_special_use_addresses() {
    for url in [
        "https://cdn.example.test/a.png",
        "https://8.8.8.8/a.png",
        "https://[2606:4700::1111]/a.png",
        // NAT64 of 8.8.8.8, as DNS64 networks resolve every IPv4 host.
        "https://[64:ff9b::808:808]/a.png",
    ] {
        assert!(LAUNCHER_ART.accepts(url), "{url} refused");
    }
    for url in [
        "http://cdn.example.test/a.png",
        "https://user:secret@cdn.example.test/a.png",
        "https://user@cdn.example.test/a.png",
        "https://127.0.0.1/a.png",
        "https://10.0.0.8/a.png",
        "https://169.254.1.1/a.png",
        "https://100.64.1.1/a.png",
        "https://198.18.0.1/a.png",
        "https://192.0.0.1/a.png",
        "https://203.0.113.5/a.png",
        "https://[::1]/a.png",
        "https://[fd00::1]/a.png",
        "https://[::ffff:127.0.0.1]/a.png",
        "https://[2002::1]/a.png",
        "https://[64:ff9b::1]/a.png",
        "https://[64:ff9b::7f00:1]/a.png",
        "https://[64:ff9b::a00:8]/a.png",
        "https://[64:ff9b:1::808:808]/a.png",
        "file:///etc/passwd",
        "not a url",
    ] {
        assert!(!LAUNCHER_ART.accepts(url), "{url} accepted");
    }
    assert!(FORM_IMAGES.accepts("http://127.0.0.1/a.png"));
    assert!(!FORM_IMAGES.accepts("ftp://example.test/a.png"));
    assert!(!STORE_ART.accepts(&format!("https://cdn.example.test/{}", "a".repeat(1024))));
}
