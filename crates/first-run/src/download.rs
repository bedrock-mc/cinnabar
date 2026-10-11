//! Resumable download of the pinned sample-pack archive with byte progress, and its bounded
//! unpack into the workspace cache.

use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use assets::{
    VanillaSource,
    carriers::{Sources, VANILLA_MANIFEST},
    vanilla_pack::{self, PackPaths, UnpackError, UnpackLimits, sha256_file},
};
use reqwest::{StatusCode, header::RANGE};

use super::runner::Cancelled;

const REPORT_INTERVAL: Duration = Duration::from_millis(100);

/// The kit's pinned pack, unpacked below `workspace`.
fn pack_paths(kit: &Path, workspace: &Path) -> Result<(VanillaSource, PackPaths)> {
    let manifest = Sources::Kit(kit.to_path_buf()).resolve(VANILLA_MANIFEST);
    let source = VanillaSource::read(&manifest)?;
    let paths = source.local_paths(workspace)?;
    Ok((source, paths))
}

/// Leaves the verified archive where [`unpack`] reads it; `progress` receives (bytes received,
/// bytes expected).
pub(super) fn fetch_archive(
    kit: &Path,
    workspace: &Path,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<()> {
    let (source, paths) = pack_paths(kit, workspace)?;
    if !source.url.starts_with("https://") {
        bail!("sample pack URL is not HTTPS: {}", source.url);
    }
    let expected = source.sha256.to_ascii_lowercase();
    let target = &paths.archive;
    if target.is_file() && sha256_file(target)? == expected {
        let len = fs::metadata(target)?.len();
        progress(len, Some(len));
        return Ok(());
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let partial = &paths.partial;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime
        .block_on(download_to(&source.url, partial, cancel, &mut progress))
        .map_err(|error| {
            if error.is::<Cancelled>() {
                error
            } else {
                error.context("Could not download the Minecraft resources. Check your internet connection, then retry")
            }
        })?;
    let actual = sha256_file(partial)?;
    if actual != expected {
        let _ = fs::remove_file(partial);
        bail!(
            "the downloaded pack failed verification (SHA-256 {actual}); retry to download it again"
        );
    }
    fs::rename(partial, target)
        .with_context(|| format!("move {} to {}", partial.display(), target.display()))
}

/// Extracts the archive [`fetch_archive`] verified into the manifest's cache directory.
pub(super) fn unpack(kit: &Path, workspace: &Path, cancel: &AtomicBool) -> Result<()> {
    let (_, paths) = pack_paths(kit, workspace)?;
    match vanilla_pack::unpack(&paths, &UnpackLimits::PINNED, &|| {
        cancel.load(Ordering::Relaxed)
    }) {
        Ok(_) => Ok(()),
        Err(UnpackError::Cancelled) => Err(Cancelled.into()),
        Err(error) => Err(error.into()),
    }
}

/// Deletes every download except the current pin's verified archive.
pub(super) fn prune(kit: &Path, workspace: &Path) {
    let keep = pack_paths(kit, workspace)
        .ok()
        .map(|(source, _)| source.archive);
    let Ok(entries) = fs::read_dir(workspace.join(vanilla_pack::DOWNLOAD_DIR)) else {
        return;
    };
    for entry in entries.flatten() {
        if keep.as_deref() != entry.file_name().to_str() {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Appends to `partial` when the server honours a range request, else starts it over.
async fn download_to(
    url: &str,
    partial: &Path,
    cancel: &AtomicBool,
    progress: &mut impl FnMut(u64, Option<u64>),
) -> Result<()> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .read_timeout(Duration::from_secs(60))
        .build()?;
    let resume_from = fs::metadata(partial).map_or(0, |meta| meta.len());
    let mut request = client.get(url);
    if resume_from > 0 {
        request = request.header(RANGE, format!("bytes={resume_from}-"));
    }
    let mut response = request
        .send()
        .await
        .context("connect to the download server")?;
    let (mut file, mut received) = match response.status() {
        StatusCode::PARTIAL_CONTENT if resume_from > 0 => {
            (OpenOptions::new().append(true).open(partial)?, resume_from)
        }
        // The partial file already holds every byte; verification decides whether it is good.
        StatusCode::RANGE_NOT_SATISFIABLE if resume_from > 0 => return Ok(()),
        status if status.is_success() => (File::create(partial)?, 0),
        status => bail!("the download server answered {status}"),
    };
    let total = response
        .content_length()
        .map(|remaining| remaining + received);
    progress(received, total);
    let mut reported = Instant::now();
    while let Some(chunk) = response
        .chunk()
        .await
        .context("the download was interrupted")?
    {
        if cancel.load(Ordering::Relaxed) {
            return Err(Cancelled.into());
        }
        file.write_all(&chunk)
            .with_context(|| format!("write {}", partial.display()))?;
        received += chunk.len() as u64;
        if reported.elapsed() >= REPORT_INTERVAL {
            progress(received, total);
            reported = Instant::now();
        }
    }
    file.sync_all()?;
    progress(received, total);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        io::{BufRead, BufReader},
        net::TcpListener,
        thread,
    };

    use sha2::{Digest, Sha256};

    use super::*;
    use crate::test_support::{Dir, write_vanilla_manifest};

    const BODY: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";

    /// Serves `BODY` once, honouring `Range: bytes=N-` only when `ranges` is set.
    fn serve_once(ranges: bool) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut start = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                    start = value.trim().trim_end_matches('-').parse().unwrap();
                }
                if line.trim().is_empty() {
                    break;
                }
            }
            let (status, body) = if ranges && start > 0 {
                ("206 Partial Content", &BODY[start..])
            } else {
                ("200 OK", BODY)
            };
            let mut stream = stream;
            write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            stream.write_all(body).unwrap();
        });
        format!("http://{address}/pack.zip")
    }

    fn download(url: &str, partial: &Path) -> Vec<(u64, Option<u64>)> {
        let mut seen = Vec::new();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime
            .block_on(download_to(
                url,
                partial,
                &AtomicBool::new(false),
                &mut |r, t| {
                    seen.push((r, t));
                },
            ))
            .unwrap();
        seen
    }

    #[test]
    fn a_partial_download_resumes_from_its_length() {
        let dir = Dir::new("download-resume");
        let partial = dir.path().join("pack.zip.partial");
        fs::write(&partial, &BODY[..10]).unwrap();
        let seen = download(&serve_once(true), &partial);
        assert_eq!(fs::read(&partial).unwrap(), BODY);
        assert_eq!(seen.first(), Some(&(10, Some(BODY.len() as u64))));
    }

    #[test]
    fn a_server_ignoring_ranges_restarts_the_file() {
        let dir = Dir::new("download-restart");
        let partial = dir.path().join("pack.zip.partial");
        fs::write(&partial, b"stale-bytes").unwrap();
        download(&serve_once(false), &partial);
        assert_eq!(fs::read(&partial).unwrap(), BODY);
    }

    #[test]
    fn pruning_keeps_only_the_current_archive() {
        let dir = Dir::new("download-prune");
        let downloads = dir.path().join(vanilla_pack::DOWNLOAD_DIR);
        fs::create_dir_all(&downloads).unwrap();
        write_vanilla_manifest(dir.path(), "https://x/new.zip", "00", "new.zip");
        for name in ["old.zip", "new.zip", "new.zip.partial"] {
            fs::write(downloads.join(name), b"x").unwrap();
        }
        prune(dir.path(), dir.path());
        let left: Vec<_> = fs::read_dir(&downloads)
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(left, ["new.zip"]);
    }

    #[test]
    fn a_verified_archive_is_reused_without_a_request() {
        let dir = Dir::new("download-reuse");
        let sha = format!("{:x}", Sha256::digest(BODY));
        write_vanilla_manifest(dir.path(), "https://127.0.0.1:9/none", &sha, "pack.zip");
        let archive = pack_paths(dir.path(), dir.path()).unwrap().1.archive;
        fs::create_dir_all(archive.parent().unwrap()).unwrap();
        fs::write(&archive, BODY).unwrap();
        let mut seen = None;
        fetch_archive(dir.path(), dir.path(), &AtomicBool::new(false), |r, t| {
            seen = Some((r, t))
        })
        .unwrap();
        assert_eq!(seen, Some((BODY.len() as u64, Some(BODY.len() as u64))));
    }
}
