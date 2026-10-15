//! Selects signed artifacts using the channel, SemVer and monotonic publication number.

use crate::{Error, Manifest, Verdict, channel::Channel, validate_artifact};
use std::cmp::Ordering;

/// Compares a verified manifest with the running build; a missing platform is not an error.
pub fn evaluate(
    manifest: &Manifest,
    channel: &str,
    platform: &str,
    current: &str,
) -> Result<Verdict, Error> {
    evaluate_build(
        manifest,
        Check {
            channel,
            platform,
            current,
            // This entry point compares versions without an installed publication number.
            current_build: manifest.build_number,
            switch_now: false,
        },
    )
}

/// Compares SemVer precedence, ordering numeric prereleases and ignoring build metadata.
pub fn newer(candidate: &str, current: &str) -> Result<bool, Error> {
    Ok(version(candidate)?
        .cmp_precedence(&version(current)?)
        .is_gt())
}

/// Orders the version core, then the shared publication number, then SemVer precedence.
/// Beta and nightly use the same packaging workflow's run number. Zero sorts before numbered
/// builds of the same core; equal numbers use prerelease precedence and ignore build metadata.
pub fn compare_builds(
    candidate: &str,
    build: u64,
    current: &str,
    installed_build: u64,
) -> Result<Ordering, Error> {
    let (candidate, current) = (version(candidate)?, version(current)?);
    let core = |version: &semver::Version| (version.major, version.minor, version.patch);
    Ok(core(&candidate)
        .cmp(&core(&current))
        .then(build.cmp(&installed_build))
        .then_with(|| candidate.cmp_precedence(&current)))
}

/// True only when the candidate follows the installed build in the shared release order.
pub fn newer_build(
    candidate: &str,
    build: u64,
    current: &str,
    installed_build: u64,
) -> Result<bool, Error> {
    Ok(compare_builds(candidate, build, current, installed_build)?.is_gt())
}

/// Parses the optional release-tag prefix without replacing SemVer's prerelease ordering.
fn version(raw: &str) -> Result<semver::Version, Error> {
    let version = semver::Version::parse(raw.strip_prefix('v').unwrap_or(raw))
        .map_err(|_| Error::Version(raw.to_owned()))?;
    if [version.major, version.minor, version.patch]
        .into_iter()
        .any(|part| part > u64::from(u32::MAX))
    {
        return Err(Error::Version(raw.to_owned()));
    }
    Ok(version)
}

/// Signed release selection, including the sole explicit exception to downgrade refusal.
#[derive(Clone, Copy)]
pub struct Check<'a> {
    pub channel: &'a str,
    pub platform: &'a str,
    pub current: &'a str,
    pub current_build: u64,
    pub switch_now: bool,
}

/// Evaluates a verified manifest using the installed build and the user's channel choice.
pub fn evaluate_build(manifest: &Manifest, check: Check<'_>) -> Result<Verdict, Error> {
    if manifest.channel != check.channel
        && !Channel::parse(check.channel)
            .is_some_and(|channel| channel.accepts_release(&manifest.channel))
    {
        return Err(Error::Channel {
            found: manifest.channel.clone(),
            expected: check.channel.to_owned(),
        });
    }
    let mut verdict = Verdict {
        current: check.current.to_owned(),
        latest: manifest.version.clone(),
        notes_url: (!manifest.notes_url.is_empty()).then(|| manifest.notes_url.clone()),
        ..Verdict::default()
    };
    if !check.switch_now
        && !newer_build(
            &manifest.version,
            manifest.build_number,
            check.current,
            check.current_build,
        )?
    {
        return Ok(verdict);
    }
    // Explicit switching still requires a valid version and a signed native artifact.
    version(&manifest.version)?;
    let Some(artifact) = manifest.artifacts.get(check.platform) else {
        return Ok(verdict);
    };
    validate_artifact(artifact)?;
    verdict.available = true;
    verdict.artifact = Some(artifact.clone());
    Ok(verdict)
}
