use std::{
    fmt,
    fs::{self, File},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use launcher::local_worlds::world::SetupState;
use reqwest::{StatusCode, header};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use url::Url;

use super::{
    Cancel, create_private_dir,
    provision::Provisioner,
    world::{WorldError, failed},
};

pub(super) const LINKS_API: &str =
    "https://net-secondary.web.minecraft-services.net/api/v1.0/download/links";
/// minecraft.net resets requests that do not name their downloader.
const AGENT: &str = "Cinnabar-local-worlds";
const MAX_ZIP_BYTES: u64 = 1 << 30;
const MAX_LINKS_BYTES: usize = 1 << 20;
#[cfg(not(target_arch = "wasm32"))]
const MAX_REDIRECTS: usize = 10;
pub(super) const DEFAULT_STALL_TIMEOUT: Duration = Duration::from_secs(30);
/// How often a pending request checks for cancellation.
const CANCEL_POLL: Duration = Duration::from_millis(200);
pub(super) const PART_SUFFIX: &str = ".zip.part";

/// The official archive URL of `version` for the `bin-<platform>` folder (`linux` or `win`).
pub(super) fn direct_url(platform: &str, version: &str) -> String {
    format!(
        "https://www.minecraft.net/bedrockdedicatedserver/bin-{platform}/bedrock-server-{version}.zip"
    )
}

/// Only https minecraft.net and minecraft-services.net hosts serve the dedicated server.
pub(super) fn official_host(url: &Url) -> bool {
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    url.scheme() == "https"
        && (host == "minecraft.net"
            || host.ends_with(".minecraft.net")
            || host.ends_with(".minecraft-services.net"))
}

/// Why a transfer ended early.
enum Fetch {
    Stalled,
    TooLarge,
    Cancelled,
    Other(String),
}

impl fmt::Display for Fetch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Stalled => "no data received for too long",
            Self::TooLarge => "dedicated server download exceeds size limit",
            Self::Cancelled => "the download was cancelled",
            Self::Other(detail) => detail.as_str(),
        })
    }
}

impl Fetch {
    fn io(error: io::Error) -> Self {
        Self::Other(error.to_string())
    }

    fn into_error(self) -> WorldError {
        match self {
            Self::Cancelled => WorldError::Cancelled,
            other => failed(other.to_string()),
        }
    }
}

#[derive(Default, Deserialize)]
struct Links {
    #[serde(default)]
    result: LinksResult,
}

#[derive(Default, Deserialize)]
struct LinksResult {
    #[serde(default)]
    links: Vec<Link>,
}

#[derive(Deserialize)]
struct Link {
    #[serde(rename = "downloadType", default)]
    download_type: String,
    #[serde(rename = "downloadUrl", default)]
    download_url: String,
}

/// Awaits `work`, failing once `stall` passes or `cancel` fires.
async fn bounded<T>(
    work: impl Future<Output = T>,
    stall: Duration,
    cancel: &Cancel,
) -> Result<T, Fetch> {
    let deadline = tokio::time::Instant::now() + stall;
    let mut work = std::pin::pin!(work);
    loop {
        if cancel.is_cancelled() {
            return Err(Fetch::Cancelled);
        }
        let now = tokio::time::Instant::now();
        if now >= deadline {
            return Err(Fetch::Stalled);
        }
        let slice = (deadline - now).min(CANCEL_POLL);
        if let Ok(value) = tokio::time::timeout(slice, work.as_mut()).await {
            return Ok(value);
        }
    }
}

impl Provisioner {
    /// A client that sends only to accepted hosts, redirects included.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn client(&self) -> Result<reqwest::Client, WorldError> {
        let allowed = self.allow_host.clone();
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::custom(move |attempt| {
                if attempt.previous().len() >= MAX_REDIRECTS {
                    attempt.error("too many redirects")
                } else if allowed(attempt.url()) {
                    attempt.follow()
                } else {
                    let host = attempt.url().host_str().unwrap_or_default().to_owned();
                    attempt.error(format!("refusing redirect to {host}"))
                }
            }))
            .build()
            .map_err(|error| failed(format!("download client: {error}")))
    }

    /// Browsers cannot police redirects, so they download no dedicated server.
    #[cfg(target_arch = "wasm32")]
    pub(super) fn client(&self) -> Result<reqwest::Client, WorldError> {
        Err(failed(
            "dedicated server downloads are unavailable in the browser",
        ))
    }

    /// Fetches `link` from byte `from` onwards; a ranged request also accepts 206 and 416.
    async fn get(
        &self,
        client: &reqwest::Client,
        link: &str,
        from: u64,
        cancel: &Cancel,
    ) -> Result<reqwest::Response, Fetch> {
        let url = Url::parse(link)
            .ok()
            .filter(|url| (self.allow_host)(url))
            .ok_or_else(|| Fetch::Other(format!("refusing non-official download URL {link:?}")))?;
        let host = url.host_str().unwrap_or_default().to_owned();
        let mut request = client.get(url).header(header::USER_AGENT, AGENT);
        if from > 0 {
            request = request.header(header::RANGE, format!("bytes={from}-"));
        }
        let response = bounded(request.send(), self.stall_timeout, cancel)
            .await?
            .map_err(|error| Fetch::Other(error.to_string()))?;
        let status = response.status();
        let ranged = from > 0
            && (status == StatusCode::PARTIAL_CONTENT
                || status == StatusCode::RANGE_NOT_SATISFIABLE);
        if status != StatusCode::OK && !ranged {
            return Err(Fetch::Other(format!("download {host}: {status}")));
        }
        Ok(response)
    }

    /// The target build's version and official archive URL.
    pub(super) async fn resolve(
        &self,
        client: &reqwest::Client,
        cancel: &Cancel,
    ) -> Result<(String, String), WorldError> {
        let (kind, folder) = if self.platform.os == "windows" {
            ("serverBedrockWindows", "win")
        } else {
            ("serverBedrockLinux", "linux")
        };
        let prefix = self.prefix();
        if !self.version.is_empty() {
            if !self.version_matches(&self.version) {
                return Err(failed(format!(
                    "server version {} does not match client version {prefix}",
                    self.version
                )));
            }
            return Ok((self.version.clone(), direct_url(folder, &self.version)));
        }
        if prefix.is_empty() {
            return Err(failed("no dedicated server version is pinned"));
        }
        let mut response = self
            .get(client, &self.links_url, 0, cancel)
            .await
            .map_err(Fetch::into_error)?;
        let mut body = Vec::new();
        while body.len() < MAX_LINKS_BYTES {
            let chunk = bounded(next_chunk(&mut response), self.stall_timeout, cancel)
                .await
                .map_err(Fetch::into_error)?
                .map_err(|error| failed(format!("read download links: {error}")))?;
            let Some(chunk) = chunk else { break };
            body.extend_from_slice(&chunk);
        }
        body.truncate(MAX_LINKS_BYTES);
        let links: Links = serde_json::from_slice(&body)
            .map_err(|error| failed(format!("decode download links: {error}")))?;
        for link in links.result.links {
            if link.download_type != kind {
                continue;
            }
            let Some(version) = zip_version(&link.download_url).map(str::to_owned) else {
                continue;
            };
            if !self.version_matches(&version) {
                return Err(failed(format!(
                    "current dedicated server is {version} but the client needs {prefix}; set an exact server version"
                )));
            }
            return Ok((version, link.download_url));
        }
        Err(failed(
            "no dedicated server download listed for this platform",
        ))
    }

    pub(super) fn downloads_dir(&self) -> PathBuf {
        self.root.join("downloads")
    }

    /// Fetches `version`'s archive into a `.zip.part` that a failed attempt leaves for a Range
    /// resume; returns the archive, its SHA-256 and its size.
    pub(super) async fn download(
        &self,
        client: &reqwest::Client,
        version: &str,
        link: &str,
        cancel: &Cancel,
    ) -> Result<(PathBuf, String, u64), WorldError> {
        self.set_op(Some(SetupState::Downloading), Some(version), 0, 0);
        let dir = self.downloads_dir();
        create_private_dir(&dir)
            .map_err(|error| failed(format!("create downloads folder: {error}")))?;
        let name = format!("bedrock-server-{version}{PART_SUFFIX}");
        let path = dir.join(&name);
        remove_stale_parts(&dir, &name);
        let mut file = open_part(&path)
            .map_err(|error| failed(format!("download dedicated server: {error}")))?;
        let mut digest = Sha256::new();
        let fetched = self
            .fetch_into(client, &mut file, &mut digest, version, link, cancel)
            .await;
        drop(file);
        match fetched {
            Ok(size) => {
                let sum = format!("{:x}", digest.finalize());
                Ok((path, sum, size))
            }
            Err(Fetch::Cancelled) => Err(WorldError::Cancelled),
            Err(error) => {
                if matches!(error, Fetch::TooLarge) {
                    let _ = fs::remove_file(path);
                }
                Err(failed(format!("download dedicated server: {error}")))
            }
        }
    }

    /// Appends the rest of `link` to `file`, hashing what is already there, and starts over when
    /// the server cannot resume.
    async fn fetch_into(
        &self,
        client: &reqwest::Client,
        file: &mut File,
        digest: &mut Sha256,
        version: &str,
        link: &str,
        cancel: &Cancel,
    ) -> Result<u64, Fetch> {
        let mut offset = hash_existing(file, digest).map_err(Fetch::io)?;
        let mut retried = false;
        loop {
            if offset > MAX_ZIP_BYTES {
                offset = 0;
            }
            if offset == 0 {
                restart(file, digest).map_err(Fetch::io)?;
            }
            let mut response = self.get(client, link, offset, cancel).await?;
            let (total, resumable) = resume_total(&response, offset);
            if !resumable {
                if retried {
                    return Err(Fetch::Other(
                        "dedicated server download cannot resume".to_owned(),
                    ));
                }
                retried = true;
                offset = 0;
                continue;
            }
            if response.status() == StatusCode::OK && offset != 0 {
                offset = 0;
                restart(file, digest).map_err(Fetch::io)?;
            }
            let total = total.unwrap_or(0);
            let mut done = offset;
            self.set_op(Some(SetupState::Downloading), Some(version), done, total);
            while let Some(chunk) = bounded(next_chunk(&mut response), self.stall_timeout, cancel)
                .await?
                .map_err(|error| Fetch::Other(error.to_string()))?
            {
                file.write_all(&chunk).map_err(Fetch::io)?;
                digest.update(&chunk);
                done += chunk.len() as u64;
                self.set_op(Some(SetupState::Downloading), Some(version), done, total);
                if done > MAX_ZIP_BYTES {
                    return Err(Fetch::TooLarge);
                }
            }
            if total > 0 && done != total {
                return Err(Fetch::Other(
                    "dedicated server download was truncated".to_owned(),
                ));
            }
            return Ok(done);
        }
    }
}

/// The response's next body chunk. A browser's reqwest cannot stream a body, and no browser gets
/// this far: its [`Provisioner::client`] always fails.
#[cfg(not(target_arch = "wasm32"))]
async fn next_chunk(response: &mut reqwest::Response) -> reqwest::Result<Option<bytes::Bytes>> {
    response.chunk().await
}

#[cfg(target_arch = "wasm32")]
async fn next_chunk(_response: &mut reqwest::Response) -> reqwest::Result<Option<bytes::Bytes>> {
    Ok(None)
}

fn open_part(path: &Path) -> io::Result<File> {
    let mut options = fs::OpenOptions::new();
    options.create(true).read(true).write(true).truncate(false);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)
}

/// Hashes the bytes already downloaded and leaves `file` positioned after them.
fn hash_existing(file: &mut File, digest: &mut Sha256) -> io::Result<u64> {
    file.seek(SeekFrom::Start(0))?;
    let mut buffer = vec![0u8; 64 << 10];
    let mut length = 0;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            return Ok(length);
        }
        digest.update(&buffer[..read]);
        length += read as u64;
    }
}

fn restart(file: &mut File, digest: &mut Sha256) -> io::Result<()> {
    *digest = Sha256::new();
    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    Ok(())
}

/// The archive's full size when known; `false` when the reply cannot continue at `offset`.
fn resume_total(response: &reqwest::Response, offset: u64) -> (Option<u64>, bool) {
    match response.status() {
        StatusCode::OK => (response.content_length(), true),
        StatusCode::PARTIAL_CONTENT => {
            let range = response
                .headers()
                .get(header::CONTENT_RANGE)
                .and_then(|value| value.to_str().ok())
                .and_then(parse_content_range);
            match range {
                Some((start, total)) => (total, start == offset),
                None => (None, false),
            }
        }
        _ => (None, false),
    }
}

/// Reads `bytes start-end/total`; the total is `None` for `*`.
fn parse_content_range(header: &str) -> Option<(u64, Option<u64>)> {
    let spec = header.strip_prefix("bytes ")?;
    let (span, size) = spec.split_once('/')?;
    let (first, _) = span.split_once('-')?;
    let start = first.parse::<u64>().ok()?;
    if size == "*" {
        return Some((start, None));
    }
    let total = size.parse::<u64>().ok()?;
    (start < total).then_some((start, Some(total)))
}

/// The build number in an official archive name, `bedrock-server-<version>.zip`.
fn zip_version(url: &str) -> Option<&str> {
    let (_, name) = url.rsplit_once("bedrock-server-")?;
    let version = name.strip_suffix(".zip")?;
    let valid = version.contains('.')
        && version
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()));
    valid.then_some(version)
}

/// Deletes interrupted downloads in `dir` other than `keep`.
pub(super) fn remove_stale_parts(dir: &Path, keep: &str) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let stale = name
            .to_str()
            .is_some_and(|name| name != keep && name.ends_with(PART_SUFFIX));
        if stale && entry.file_type().is_ok_and(|kind| kind.is_file()) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
#[path = "download_tests.rs"]
mod tests;
