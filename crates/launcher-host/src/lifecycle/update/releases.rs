//! Authenticated channel candidates and their independent manifest caches.

use super::Channel;
use anyhow::{Context, Result, ensure};
use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};
use time::OffsetDateTime;
use update_installer::stage::Selection;
use update_manifest::{TrustedKeys, select::Check};

/// Fetches the selected channel and its optional companion, tolerating companion failures.
pub(super) async fn select(
    root: &Path,
    keys: &TrustedKeys,
    check: Check<'_>,
    prefer_cached: bool,
    now: OffsetDateTime,
    mut fetch_channel: impl AsyncFnMut(Channel) -> Result<Vec<u8>>,
) -> Result<Selection> {
    let channel = Channel::parse(check.channel).context("unknown update channel")?;
    let primary = load(root, keys, check, prefer_cached, now, &mut fetch_channel).await;
    let additional = if let Some(channel) = channel.additional_channel() {
        load(
            root,
            keys,
            Check {
                channel: channel.name(),
                ..check
            },
            prefer_cached,
            now,
            &mut fetch_channel,
        )
        .await
        .ok()
    } else {
        None
    };
    newest(primary, additional)
}

/// Selects the same winner from authenticated caches when a network check is not due.
pub(super) fn cached(
    root: &Path,
    keys: &TrustedKeys,
    check: Check<'_>,
    now: OffsetDateTime,
) -> Option<Selection> {
    let channel = Channel::parse(check.channel)?;
    let primary = cached_channel(&cache_path(root, channel), keys, check, now)
        .context("no cached channel release");
    let additional = channel.additional_channel().and_then(|channel| {
        cached_channel(
            &cache_path(root, channel),
            keys,
            Check {
                channel: channel.name(),
                ..check
            },
            now,
        )
    });
    newest(primary, additional).ok()
}

/// Reads or fetches one exact publishing channel before updating that channel's cache.
async fn load(
    root: &Path,
    keys: &TrustedKeys,
    check: Check<'_>,
    prefer_cached: bool,
    now: OffsetDateTime,
    fetch_channel: &mut impl AsyncFnMut(Channel) -> Result<Vec<u8>>,
) -> Result<Selection> {
    let channel = Channel::parse(check.channel).context("unknown update channel")?;
    let path = cache_path(root, channel);
    if prefer_cached && let Some(selected) = cached_channel(&path, keys, check, now) {
        return Ok(selected);
    }
    let envelope = fetch_channel(channel).await?;
    let selected = verify_channel(&envelope, keys, check, now)?;
    let _ = fs::write(path, &envelope);
    Ok(selected)
}

/// Chooses the newer verified release without making an optional channel a failure condition.
fn newest(primary: Result<Selection>, additional: Option<Selection>) -> Result<Selection> {
    let Some(additional) = additional else {
        return primary;
    };
    let Ok(primary) = primary else {
        return Ok(additional);
    };
    if update_manifest::select::newer_build(
        &additional.manifest.version,
        additional.manifest.build_number,
        &primary.manifest.version,
        primary.manifest.build_number,
    )? {
        Ok(additional)
    } else {
        Ok(primary)
    }
}

/// Binds an envelope to its publishing endpoint even when the preference accepts two channels.
fn verify_channel(
    envelope: &[u8],
    keys: &TrustedKeys,
    check: Check<'_>,
    now: OffsetDateTime,
) -> Result<Selection> {
    let selected = Selection::verify(
        envelope,
        keys,
        Check {
            switch_now: true,
            ..check
        },
        now,
    )?;
    ensure!(
        selected.manifest.channel == check.channel,
        "manifest channel mismatch"
    );
    Ok(selected)
}

/// Reuses only authenticated, unexpired cached releases for this exact publishing channel.
pub(super) fn cached_channel(
    path: &Path,
    keys: &TrustedKeys,
    check: Check<'_>,
    now: OffsetDateTime,
) -> Option<Selection> {
    verify_channel(&read_cached(path)?, keys, check, now).ok()
}

/// Returns the independent cache path for one publishing channel.
pub(super) fn cache_path(root: &Path, channel: Channel) -> PathBuf {
    root.join(format!("manifest-{}.json", channel.name()))
}

/// Reads a cached envelope no larger than a fetched one may be.
pub(super) fn read_cached(path: &Path) -> Option<Vec<u8>> {
    let mut cached = Vec::new();
    File::open(path)
        .and_then(|file| {
            file.take(update_manifest::MAX_ENVELOPE_BYTES as u64 + 1)
                .read_to_end(&mut cached)
        })
        .ok()?;
    Some(cached)
}

#[cfg(test)]
mod tests;
