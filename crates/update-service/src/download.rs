//! HTTPS manifest fetches and the resumable, digest-named artifact download.

use anyhow::{Result, ensure};
use std::{fs, path::Path, time::Duration};
use update_installer::stage::{Download, Selection};

const FETCH_TIMEOUT: Duration = Duration::from_secs(15);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// A signed manifest can outlive the download it names; fetch the channel again.
#[derive(Debug, thiserror::Error)]
#[error("the update download is no longer published")]
pub(crate) struct MissingArtifact;

/// A missing served document or payload warrants one refresh of the signed channel manifest.
pub(crate) fn check_missing(status: reqwest::StatusCode) -> Result<()> {
    if status == reqwest::StatusCode::NOT_FOUND {
        return Err(MissingArtifact.into());
    }
    Ok(())
}

/// Refreshes a missing payload's manifest once, leaving every other failure to the caller.
pub(crate) fn retry_missing(
    mut attempt: impl FnMut() -> Result<()>,
    active: impl FnOnce() -> bool,
) -> Result<()> {
    let result = attempt();
    if result
        .as_ref()
        .is_err_and(|error| error.is::<MissingArtifact>())
        && active()
    {
        attempt()
    } else {
        result
    }
}

/// Shares the HTTPS-only policy and idle deadlines between checks and range requests.
pub(crate) fn http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .https_only(true)
        .connect_timeout(FETCH_TIMEOUT)
        .read_timeout(FETCH_TIMEOUT)
        .build()?)
}

/// Bounds the signed document separately from the much larger release artifact.
pub(crate) async fn fetch(client: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    let mut response = client
        .get(url)
        .header(reqwest::header::CACHE_CONTROL, "no-cache")
        .timeout(FETCH_TIMEOUT)
        .send()
        .await?;
    check_missing(response.status())?;
    ensure!(
        response.status() == reqwest::StatusCode::OK,
        "manifest download failed: {}",
        response.status()
    );
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            body.len() + chunk.len() <= update_manifest::MAX_ENVELOPE_BYTES,
            "manifest exceeds size limit"
        );
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// Streams `selected`'s artifact into `root`, resuming a partial file, and publishes the
/// finished stage at `destination`. `progress` returns false to cancel.
pub(crate) async fn artifact(
    client: &reqwest::Client,
    root: &Path,
    destination: &Path,
    selected: Selection,
    mut progress: impl FnMut(u64, u64) -> bool,
) -> Result<std::path::PathBuf> {
    let artifact = selected.artifact.clone();
    let size = artifact.size as u64;
    let mut download = Download::begin(root, selected)?;
    // Retain only the current partial transfer; another digest can restart safely.
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if let Some(digest) = name.strip_suffix(".part")
            && digest.len() == 64
            && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
            && !digest.eq_ignore_ascii_case(&artifact.sha256)
            && entry.file_type()?.is_file()
        {
            fs::remove_file(entry.path())?;
        }
    }
    let offset = download.offset();
    if offset < size {
        let mut request = client.get(&artifact.url).timeout(DOWNLOAD_TIMEOUT);
        if offset != 0 {
            request = request.header(reqwest::header::RANGE, format!("bytes={offset}-"));
        }
        let mut response = request.send().await?;
        check_missing(response.status())?;
        if response.status() == reqwest::StatusCode::PARTIAL_CONTENT {
            let expected = format!("bytes {offset}-{}/{size}", size - 1);
            ensure!(
                response
                    .headers()
                    .get(reqwest::header::CONTENT_RANGE)
                    .and_then(|value| value.to_str().ok())
                    == Some(expected.as_str()),
                "invalid resumed range"
            );
        } else {
            ensure!(
                response.status() == reqwest::StatusCode::OK,
                "update download failed: {}",
                response.status()
            );
            if offset != 0 {
                download.restart()?;
            }
        }
        if let Some(length) = response.content_length() {
            ensure!(
                length == size - download.offset(),
                "update download length mismatch"
            );
        }
        ensure!(progress(download.offset(), size), "update cancelled");
        while let Some(chunk) = response.chunk().await? {
            download.write(&chunk)?;
            ensure!(progress(download.offset(), size), "update cancelled");
        }
    }
    download.finish(destination)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_http_client_refuses_plaintext_before_connecting() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/update.json", listener.local_addr().unwrap());
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        assert!(
            runtime
                .block_on(fetch(&http_client().unwrap(), &url))
                .is_err()
        );
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }

    #[test]
    fn a_missing_payload_refreshes_the_channel_once() {
        let mut fetched = 0;
        retry_missing(
            || {
                fetched += 1;
                check_missing(if fetched == 1 {
                    reqwest::StatusCode::NOT_FOUND
                } else {
                    reqwest::StatusCode::OK
                })
            },
            || true,
        )
        .unwrap();
        assert_eq!(fetched, 2);
        fetched = 0;
        assert!(
            retry_missing(
                || {
                    fetched += 1;
                    check_missing(reqwest::StatusCode::NOT_FOUND)
                },
                || true
            )
            .is_err()
        );
        assert_eq!(fetched, 2);
    }

    #[test]
    fn cancellation_and_other_failures_do_not_refetch_the_manifest() {
        for missing in [false, true] {
            let mut fetched = 0;
            assert!(
                retry_missing(
                    || {
                        fetched += 1;
                        if missing {
                            Err(MissingArtifact.into())
                        } else {
                            anyhow::bail!("connection failed")
                        }
                    },
                    || false
                )
                .is_err()
            );
            assert_eq!(fetched, 1);
        }
    }
}
