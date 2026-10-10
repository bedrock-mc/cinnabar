use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::atomic::AtomicUsize,
    time::Duration,
};

use super::super::{FORM_IMAGES, LAUNCHER_ART, STORE_ART, public_ip};
use super::*;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nrest";

/// A loopback surface: the launcher policy without the public-address rule.
fn local(surface: Surface) -> Surface {
    Surface {
        public_only: false,
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
    let results = runtime().block_on(cache.fetch_all(urls.clone()));
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

#[test]
fn public_surfaces_accept_only_https_without_credentials_to_public_hosts() {
    for url in [
        "https://cdn.example.test/a.png",
        "https://93.184.216.34/a.png",
        "https://[2606:2800:220:1::1]/a.png",
    ] {
        assert!(LAUNCHER_ART.accepts(url), "{url} refused");
    }
    for url in [
        "http://cdn.example.test/a.png",
        "https://user:secret@cdn.example.test/a.png",
        "https://user@cdn.example.test/a.png",
        "https://127.0.0.1/a.png",
        "https://10.0.0.8/a.png",
        "https://192.168.1.2/a.png",
        "https://169.254.1.1/a.png",
        "https://[::1]/a.png",
        "https://[fd00::1]/a.png",
        "https://[::ffff:127.0.0.1]/a.png",
        "file:///etc/passwd",
        "not a url",
    ] {
        assert!(!LAUNCHER_ART.accepts(url), "{url} accepted");
    }
    assert!(FORM_IMAGES.accepts("http://127.0.0.1/a.png"));
    assert!(!FORM_IMAGES.accepts("ftp://example.test/a.png"));
    assert!(!STORE_ART.accepts(&format!("https://cdn.example.test/{}", "a".repeat(1024))));
}

#[test]
fn only_public_addresses_resolve_for_public_surfaces() {
    for ip in ["8.8.8.8", "2606:4700::1111"] {
        assert!(public_ip(ip.parse().unwrap()), "{ip}");
    }
    for ip in [
        "127.0.0.1",
        "0.1.2.3",
        "172.16.0.1",
        "224.0.0.1",
        "255.255.255.255",
        "fe80::1",
        "::",
    ] {
        assert!(!public_ip(ip.parse().unwrap()), "{ip}");
    }
}
