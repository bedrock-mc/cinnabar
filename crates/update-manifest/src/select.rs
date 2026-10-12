//! Selects signed artifacts using the channel, SemVer and monotonic publication number.

use crate::{Error, Manifest, Verdict, validate_artifact};

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
            current_build: 0,
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

/// Orders app versions first, then signed build numbers for successive builds of that version.
pub fn newer_build(
    candidate: &str,
    build: u64,
    current: &str,
    installed_build: u64,
) -> Result<bool, Error> {
    let (candidate, current) = (version(candidate)?, version(current)?);
    let core = |version: &semver::Version| (version.major, version.minor, version.patch);
    if core(&candidate) != core(&current) {
        return Ok(core(&candidate) > core(&current));
    }
    if installed_build != 0 {
        return Ok(build > installed_build);
    }
    Ok(candidate.cmp_precedence(&current).is_gt())
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
    if manifest.channel != check.channel {
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
