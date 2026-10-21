//! Bounded same-origin catalog downloads and content-addressed disk caching.
use cosmetics::{Asset, Catalog};
use reqwest::{Client, StatusCode};
use std::{
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

/// Returns the local cosmetics cache, separate from imported skins and server packs.
pub fn cache_dir(layout: &launcher::install_layout::InstallLayout) -> PathBuf {
    layout.user_data_root.join("cosmetics")
}

/// Reads a bounded cached catalog; unavailable or malformed optional content leaves the wardrobe empty.
pub fn cached(root: &Path) -> Option<Catalog> {
    let mut bytes = Vec::new();
    std::fs::File::open(root.join("catalog.json"))
        .ok()?
        .take(cosmetics::MAX_ASSET_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > cosmetics::MAX_ASSET_BYTES {
        return None;
    }
    let catalog: Catalog = serde_json::from_slice(&bytes).ok()?;
    (catalog.items.len() <= cosmetics::MAX_ITEMS && catalog.emotes.len() <= cosmetics::MAX_ITEMS)
        .then_some(catalog)
}

/// Downloads a fresh catalog and each supported item's verified assets on a wardrobe worker.
pub fn refresh(root: &Path) -> Result<(), String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    runtime.block_on(refresh_at(root, bridge::contract::CINNABAR_API_BASE_URL))
}

/// Fetches only the configured API origin; redirects and catalog-selected foreign hosts are refused.
async fn refresh_at(root: &Path, origin: &str) -> Result<(), String> {
    std::fs::create_dir_all(root).map_err(|error| error.to_string())?;
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| error.to_string())?;
    let origin = url::Url::parse(origin).map_err(|error| error.to_string())?;
    let mut request = client.get(
        origin
            .join("/v1/cosmetics/catalog")
            .map_err(|error| error.to_string())?,
    );
    if cached(root).is_some()
        && let Ok(etag) = std::fs::read_to_string(root.join("etag"))
    {
        request = request.header("If-None-Match", etag);
    }
    let response = request.send().await.map_err(|error| error.to_string())?;
    let unchanged = response.status() == StatusCode::NOT_MODIFIED;
    let etag = response
        .headers()
        .get("etag")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let (catalog, body) = if unchanged {
        (
            cached(root).ok_or("missing cached cosmetics catalog")?,
            None,
        )
    } else {
        let body = bounded(response, cosmetics::MAX_ASSET_BYTES).await?;
        let catalog: Catalog = serde_json::from_slice(&body).map_err(|error| error.to_string())?;
        if catalog.items.len() > cosmetics::MAX_ITEMS || catalog.emotes.len() > cosmetics::MAX_ITEMS
        {
            return Err("cosmetics catalog is too large".into());
        }
        (catalog, Some(body))
    };
    let assets = catalog
        .items
        .iter()
        .filter(|item| cosmetics::supported(item))
        .flat_map(|item| {
            [&item.assets.geometry, &item.assets.texture]
                .into_iter()
                .chain(item.assets.animation.iter())
                .map(|asset| (asset, false))
        })
        .chain(
            catalog
                .emotes
                .iter()
                .filter(|item| {
                    let supported = cosmetics::emotes::supported(item);
                    if !supported {
                        cosmetics::emotes::reject();
                        tracing::warn!("unsupported catalog emote skipped");
                    }
                    supported
                })
                .map(|item| (&item.clip, true)),
        );
    for (asset, emote) in assets {
        let fetched: Result<(), String> = async {
            if read_asset(root, asset).is_some() {
                return Ok(());
            }
            let url = origin.join(&asset.url).map_err(|error| error.to_string())?;
            if url.origin() != origin.origin() {
                return if emote {
                    Err("foreign emote asset origin".into())
                } else {
                    Ok(())
                };
            }
            let response = client
                .get(url)
                .send()
                .await
                .map_err(|error| error.to_string())?;
            let bytes = bounded(response, asset.bytes).await?;
            if !asset.verifies(&bytes) {
                return Err("cosmetic asset digest mismatch".into());
            }
            write_atomic(&root.join(&asset.sha256), &bytes)
        }
        .await;
        if let Err(error) = fetched {
            if emote {
                cosmetics::emotes::reject();
                tracing::warn!(%error, "catalog emote skipped");
            } else {
                return Err(error);
            }
        }
    }
    if let Some(body) = body {
        write_atomic(&root.join("catalog.json"), &body)?;
    }
    if let Some(etag) = etag {
        write_atomic(&root.join("etag"), etag.as_bytes())?;
    }
    Ok(())
}

/// Reads only bounded, hash-verified content from disk, including after a process restart.
pub fn read_asset(root: &Path, asset: &Asset) -> Option<Vec<u8>> {
    if asset.bytes > cosmetics::MAX_ASSET_BYTES
        || asset.sha256.len() != 64
        || !asset.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return None;
    }
    let mut bytes = Vec::new();
    std::fs::File::open(root.join(&asset.sha256))
        .ok()?
        .take(asset.bytes as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    asset.verifies(&bytes).then_some(bytes)
}

/// Streams a response into a hard byte bound rather than trusting its Content-Length.
async fn bounded(mut response: reqwest::Response, max: usize) -> Result<Vec<u8>, String> {
    if !response.status().is_success()
        || response
            .content_length()
            .is_some_and(|size| size > max as u64)
    {
        return Err("cosmetic download unavailable or too large".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| error.to_string())? {
        if bytes.len().saturating_add(chunk.len()) > max {
            return Err("cosmetic download is too large".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

/// Publishes one complete cache file; failed writes leave the previous version intact.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = tempfile::NamedTempFile::new_in(path.parent().ok_or("missing cache directory")?)
        .map_err(|error| error.to_string())?;
    std::io::Write::write_all(&mut file, bytes).map_err(|error| error.to_string())?;
    file.persist(path).map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    #[test]
    fn disk_cache_rejects_corruption_oversize_and_path_traversal() {
        let root = tempfile::tempdir().unwrap();
        let bytes = b"original cosmetic asset";
        let mut asset = Asset {
            url: String::new(),
            sha256: format!("{:x}", Sha256::digest(bytes)),
            bytes: bytes.len(),
        };
        write_atomic(&root.path().join(&asset.sha256), bytes).unwrap();
        assert_eq!(read_asset(root.path(), &asset).unwrap(), bytes);
        std::fs::write(root.path().join(&asset.sha256), b"corrupt").unwrap();
        assert!(read_asset(root.path(), &asset).is_none());
        asset.sha256 = "../catalog.json".into();
        assert!(read_asset(root.path(), &asset).is_none());
        std::fs::write(
            root.path().join("catalog.json"),
            vec![b' '; cosmetics::MAX_ASSET_BYTES + 1],
        )
        .unwrap();
        assert!(cached(root.path()).is_none());
    }
}

#[cfg(test)]
mod http_tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };

    /// Builds a content-addressed synthetic asset for cache exchange tests.
    fn asset(bytes: &[u8]) -> Asset {
        let sha256 = format!("{:x}", Sha256::digest(bytes));
        Asset {
            url: format!("/v1/cosmetics/assets/{sha256}"),
            sha256,
            bytes: bytes.len(),
        }
    }

    #[test]
    fn catalog_refresh_reuses_etags_and_verified_assets() {
        let root = tempfile::tempdir().unwrap();
        let geometry = b"synthetic geometry";
        let texture = b"synthetic texture";
        let clip = b"synthetic clip";
        let clip_asset = asset(clip);
        let geometry_asset = asset(geometry);
        let texture_asset = asset(texture);
        let catalog = Catalog {
            version: 1,
            emotes: vec![cosmetics::emotes::Descriptor {
                id: "fixture:cached_clip".into(),
                display_name: "Clip fixture".into(),
                free: true,
                clip: clip_asset.clone(),
            }],
            items: vec![cosmetics::Descriptor {
                id: "fixture:cached".into(),
                slot: "head".into(),
                display_name: "Cache fixture".into(),
                tags: vec![],
                free: true,
                assets: cosmetics::Assets {
                    geometry: geometry_asset.clone(),
                    texture: texture_asset.clone(),
                    animation: None,
                },
                geometry_id: "geometry.fixture".into(),
                attachment_bone: "head".into(),
                offset: [0.0; 3],
                render_flags: cosmetics::RenderFlags {
                    double_sided: false,
                    emissive: false,
                },
            }],
        };
        let body = serde_json::to_vec(&catalog).unwrap();
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for index in 0..5 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") && request.len() < 4096 {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                }
                let request = String::from_utf8(request).unwrap();
                let response = if request.starts_with("GET /v1/cosmetics/catalog ") {
                    if index == 4 {
                        assert!(
                            request
                                .to_ascii_lowercase()
                                .contains("if-none-match: \"fixture\"")
                        );
                        ("304 Not Modified", &[][..])
                    } else {
                        ("200 OK", body.as_slice())
                    }
                } else if request.starts_with(&format!("GET {} ", geometry_asset.url)) {
                    ("200 OK", &geometry[..])
                } else if request.starts_with(&format!("GET {} ", texture_asset.url)) {
                    ("200 OK", &texture[..])
                } else if request.starts_with(&format!("GET {} ", clip_asset.url)) {
                    ("200 OK", &clip[..])
                } else {
                    ("404 Not Found", &[][..])
                };
                write!(stream, "HTTP/1.1 {}\r\nContent-Length: {}\r\nETag: \"fixture\"\r\nConnection: close\r\n\r\n", response.0, response.1.len()).unwrap();
                stream.write_all(response.1).unwrap();
                requests.push(request);
            }
            requests
        });
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(refresh_at(root.path(), &origin)).unwrap();
        runtime.block_on(refresh_at(root.path(), &origin)).unwrap();
        assert_eq!(cached(root.path()).unwrap(), catalog);
        assert_eq!(
            read_asset(root.path(), &catalog.emotes[0].clip).unwrap(),
            clip
        );
        assert_eq!(server.join().unwrap().len(), 5);
    }
}
