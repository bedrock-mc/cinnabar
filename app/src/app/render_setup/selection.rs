//! Probes APIs separately, ranks eligible adapters, and exhausts device candidates.

use anyhow::{Result, bail};
use launcher::graphics_backend::{Adapter, Backend, Platform, blocklist, default_order};
use launcher_host::renderer_startup::{Attempt, Stage, StartupGuard};

use super::device::log;

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

/// Orders hardware classes without letting an earlier API's slower adapter win.
fn device_rank(kind: wgpu::DeviceType) -> u8 {
    match kind {
        wgpu::DeviceType::DiscreteGpu => 0,
        wgpu::DeviceType::IntegratedGpu => 1,
        wgpu::DeviceType::VirtualGpu => 2,
        wgpu::DeviceType::Cpu => 3,
        wgpu::DeviceType::Other => 4,
    }
}

/// Uses device type first, then the adapter vendor's default backend order.
pub(super) fn rank<T>(candidates: &mut [Candidate<T>], platform: Platform) {
    candidates.sort_by_key(|candidate| {
        let order = default_order(platform, &candidate.identity);
        (
            device_rank(candidate.device_type),
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
