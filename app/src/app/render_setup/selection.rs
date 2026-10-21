//! Probes APIs separately, ranks eligible adapters, and exhausts device candidates.

use anyhow::{Context, Result, bail};
use launcher::graphics_backend::{Adapter, Backend, Platform, blocklist, default_order};
use launcher_host::renderer_startup::{Attempt, Stage, StartupGuard};

use super::device::log;

/// A saved Video choice this launch could not use, so Auto chose without it.
#[derive(Debug, Eq, PartialEq)]
pub(super) struct VideoFallback {
    pub requested: Backend,
    pub reason: String,
}

/// Tries a saved Video choice, then Auto without that API for this launch; the environment is
/// strict. A `crashed` choice is skipped until the setting changes. `attempt` receives the Video
/// choice and the APIs this launch excludes.
pub(super) fn with_video_fallback<T>(
    environment: bool,
    video: Option<Backend>,
    crashed: Option<Backend>,
    mut attempt: impl FnMut(Option<Backend>, &[Backend]) -> Result<T>,
) -> Result<(T, Option<VideoFallback>)> {
    let Some(requested) = video.filter(|_| !environment) else {
        return attempt(video, &[]).map(|renderer| (renderer, None));
    };
    let reason = if crashed == Some(requested) {
        "it stopped an earlier launch before the first frame".to_owned()
    } else {
        match attempt(video, &[]) {
            Ok(renderer) => return Ok((renderer, None)),
            Err(error) => format!("{error:#}"),
        }
    };
    log(format_args!(
        "WARNING: Graphics API {requested:?} from the Video setting cannot be used: {reason}; \
         trying Auto without it for this launch"
    ));
    let renderer = attempt(None, &[requested]).with_context(|| {
        format!("Graphics API {requested:?} cannot be used ({reason}), and Auto also failed")
    })?;
    Ok((renderer, Some(VideoFallback { requested, reason })))
}

/// States the requested API, the API in use and why, for the startup log.
pub(super) fn summary(
    environment: Option<&[Backend]>,
    video: Option<Backend>,
    chosen: Backend,
    fallback: Option<&VideoFallback>,
) -> String {
    let requested = match (environment, video) {
        (Some(apis), _) => format!("{apis:?} from WGPU_BACKEND"),
        (None, Some(api)) => format!("{api:?} from the Video setting"),
        (None, None) => "Auto".to_owned(),
    };
    let why = match fallback {
        Some(fallback) => format!(
            "Auto fallback because {:?} cannot be used: {}",
            fallback.requested, fallback.reason
        ),
        None if environment.is_some() || video.is_some() => "the requested API".to_owned(),
        None => "the first working API in automatic order".to_owned(),
    };
    format!("Renderer API: requested {requested}; using {chosen:?}; reason: {why}")
}

/// Retries exhausted automatic selection once with previously excluded APIs restored.
pub(super) fn with_recovery<T>(
    guard: &mut StartupGuard,
    allowed: &[Backend],
    explicit: bool,
    mut attempt: impl FnMut(&mut StartupGuard) -> Result<T>,
) -> Result<T> {
    loop {
        match attempt(guard) {
            Ok(selected) => return Ok(selected),
            Err(error) => {
                if explicit || !guard.retry_exclusions(allowed)? {
                    return Err(error);
                }
            }
        }
    }
}

/// An enumerated adapter and the native resources needed to try it.
pub(super) struct Candidate<T> {
    pub backend: Backend,
    pub identity: Adapter,
    pub device_type: wgpu::DeviceType,
    pub native: T,
}

impl<T> Candidate<T> {
    /// Attributes native operations to this candidate's actual API and hardware.
    fn attempt(&self, stage: Stage, explicit: bool) -> Attempt {
        Attempt {
            backend: self.backend,
            adapter: Some(self.identity.clone()),
            stage,
            explicit,
        }
    }
}

/// Guards instance creation, enumeration and rejected-adapter cleanup for exactly one API.
pub(super) fn probe_backend<T>(
    guard: &mut StartupGuard,
    backend: Backend,
    explicit: bool,
    probe: impl FnOnce() -> Result<T>,
) -> Result<T> {
    guard.begin(Attempt {
        backend,
        adapter: None,
        stage: Stage::EnumeratingAdapters,
        explicit,
    })?;
    let result = probe();
    guard.failed()?;
    result
}

/// Restricts names and software requests before applying automatic driver blocklist rules.
pub(super) fn eligible<T>(
    candidate: &Candidate<T>,
    platform: Platform,
    name: Option<&str>,
    software: bool,
    explicit: bool,
) -> bool {
    name.is_none_or(|name| {
        candidate
            .identity
            .name
            .to_lowercase()
            .contains(&name.to_lowercase())
    }) && (!software || candidate.device_type == wgpu::DeviceType::Cpu)
        && (explicit
            || !blocklist::rules().iter().any(|rule| {
                rule.backend == candidate.backend && rule.matches(platform, &candidate.identity)
            }))
}

/// Honors low-power hardware preference while keeping software behind unclassified hardware.
fn device_rank(kind: wgpu::DeviceType, preference: wgpu::PowerPreference) -> u8 {
    let low_power = preference == wgpu::PowerPreference::LowPower;
    match kind {
        wgpu::DeviceType::DiscreteGpu => u8::from(low_power),
        wgpu::DeviceType::IntegratedGpu => u8::from(!low_power),
        wgpu::DeviceType::VirtualGpu => 2,
        wgpu::DeviceType::Other => 3,
        wgpu::DeviceType::Cpu => 4,
    }
}

/// Ranks device types by power preference, then uses the vendor's default backend order.
pub(super) fn rank<T>(
    candidates: &mut [Candidate<T>],
    platform: Platform,
    preference: wgpu::PowerPreference,
) {
    candidates.sort_by_key(|candidate| {
        let order = default_order(platform, &candidate.identity);
        (
            device_rank(candidate.device_type, preference),
            order
                .iter()
                .position(|backend| *backend == candidate.backend),
        )
    });
}

/// Tries every ranked candidate, retaining a sentinel for native creation and cleanup.
pub(super) fn try_candidates<T, R>(
    candidates: Vec<Candidate<T>>,
    guard: &mut StartupGuard,
    explicit: bool,
    mut create: impl FnMut(Candidate<T>) -> Result<R>,
) -> Result<R> {
    let mut remaining = candidates.into_iter();
    while let Some(candidate) = remaining.next() {
        let pending = candidate.attempt(Stage::FirstPresentation, explicit);
        guard.begin(candidate.attempt(Stage::CreatingDevice, explicit))?;
        match create(candidate) {
            Ok(renderer) => {
                // Unused native resources must also be dropped under their own API's sentinel.
                for unused in remaining {
                    probe_backend(guard, unused.backend, explicit, || {
                        drop(unused);
                        Ok(())
                    })?;
                }
                guard.begin(pending)?;
                return Ok(renderer);
            }
            Err(error) => {
                log(format_args!(
                    "Renderer {:?} initialization failed: {error:#}",
                    pending.backend
                ));
                guard.failed()?;
            }
        }
    }
    bail!(
        "No safe supported graphics adapter remains. See client.log and crashes; update GPU drivers or choose a Graphics API to retry."
    )
}

#[cfg(test)]
mod tests;
