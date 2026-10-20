//! URL download and image-publication regressions with local HTTP responses.

use std::io::Cursor;
use ui_engine::atlas::ServerAtlas;

/// Serves one image request and records its complete request target.
fn image_request(bytes: Vec<u8>, target: &'static str, content_type: &'static str) -> String {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}{target}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0; 4096];
        let count = stream.read(&mut request).unwrap();
        let request = String::from_utf8_lossy(&request[..count]);
        assert_eq!(request.split_whitespace().nth(1), Some(target));
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", bytes.len()).unwrap();
        let _ = stream.write_all(&bytes);
    });
    url
}

/// Redirects one request to the image URL without discarding its query string.
fn redirect_to(url: &str) -> String {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let source = format!("http://{}/redirect", listener.local_addr().unwrap());
    let target = url.to_owned();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0; 4096];
        let _ = stream.read(&mut request);
        write!(stream, "HTTP/1.1 302 Found\r\nLocation: {target}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
    });
    source
}

#[test]
fn form_url_images_preserve_queries_and_decode_common_formats() {
    use ui_engine::sources::{ImageWork, RemoteImage};
    for (format, target, content_type) in [
        (image::ImageFormat::Png, "/icon.png", "image/png"),
        (image::ImageFormat::Jpeg, "/icon.jpg", "image/jpeg"),
        (image::ImageFormat::Gif, "/icon.gif", "image/gif"),
        (image::ImageFormat::Bmp, "/icon.bmp", "image/bmp"),
        (
            image::ImageFormat::Png,
            "/attachments/123/icon.png?ex=abc&is=def&hm=0123",
            "image/png",
        ),
        (
            image::ImageFormat::Jpeg,
            "/raw/icon?format=jpg&size=64",
            "application/octet-stream",
        ),
    ] {
        let mut bytes = Cursor::new(Vec::new());
        let image = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            64,
            64,
            image::Rgb([40, 120, 200]),
        ));
        let image = if format == image::ImageFormat::Gif {
            image::DynamicImage::ImageRgba8(image.into_rgba8())
        } else {
            image
        };
        image.write_to(&mut bytes, format).unwrap();
        let url = image_request(bytes.into_inner(), target, content_type);
        let url = if target.starts_with("/raw/") {
            redirect_to(&url)
        } else {
            url
        };
        let remote = super::texture_sources::downloads();
        let mut atlas = ServerAtlas::new(&[], None, 1)
            .with_fallbacks(None, Some(remote.clone()))
            .with_image_work(ImageWork::inline());
        test_time::eventually("form image download settles", || {
            remote.state(&url) != RemoteImage::Loading
        });
        assert!(
            matches!(remote.state(&url), RemoteImage::Ready(_)),
            "{target}"
        );
        atlas.require([url.as_str()]);
        assert!(atlas.placement(&url).is_some(), "{target} renders");
        assert_eq!(atlas.fallback_size(&url), Some([64.0, 64.0]));
    }
}

/// Encodes a valid, incompressible image larger than the old download allowance.
fn large_png() -> Vec<u8> {
    let mut image = image::RgbImage::new(1024, 1024);
    let mut noise = 0x1234_5678u32;
    for pixel in image.pixels_mut() {
        for channel in &mut pixel.0 {
            noise ^= noise << 13;
            noise ^= noise >> 17;
            noise ^= noise << 5;
            *channel = noise as u8;
        }
    }
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    bytes.into_inner()
}

#[test]
fn form_url_images_larger_than_two_megabytes_reach_the_atlas() {
    use ui_engine::sources::{ImageWork, RemoteImage};
    let bytes = large_png();
    assert!(bytes.len() > 2 * 1024 * 1024);
    let url = image_request(bytes, "/large.png?size=1024", "image/png");
    let remote = super::texture_sources::downloads();
    test_time::eventually("large form image download settles", || {
        remote.state(&url) != RemoteImage::Loading
    });
    assert!(
        matches!(remote.state(&url), RemoteImage::Ready(_)),
        "a valid PNG must reach the image decoder"
    );
    let mut atlas = ServerAtlas::new(&[], None, 1)
        .with_fallbacks(None, Some(remote))
        .with_image_work(ImageWork::inline());
    atlas.require([url.as_str()]);
    assert!(atlas.placement(&url).is_some());
    assert_eq!(atlas.fallback_size(&url), Some([1024.0, 1024.0]));
}
