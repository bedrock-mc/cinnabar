//! Stability policy shared by startup, probes and the persisted Video setting.

use serde::{Deserialize, Serialize};
use std::time::Duration;

pub mod blocklist;
#[cfg(test)]
mod tests;

/// Intel's PCI vendor identifier, used only with an integrated device classification.
pub const INTEL_PCI_VENDOR: u32 = 0x8086;
/// Maximum wall time allowed for an isolated driver call, including presentation.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
/// Private executable dispatch flags, handled before setup or asset loading.
pub const PROBE_FLAG: &str = "--gpu-probe";
pub const INVENTORY_FLAG: &str = "--gpu-inventory";

// Keep the enum, names and complete registry together so a new API cannot be omitted.
macro_rules! backends {
    ($($variant:ident => ($name:literal, $label:literal)),+ $(,)?) => {
        /// APIs understood by the graphics policy; platform support is defined below.
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
        #[serde(rename_all = "lowercase")]
        pub enum Backend { $($variant),+ }
        impl Backend {
            /// Every backend, without imposing a platform preference.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];
            /// The wgpu environment spelling for a single API.
            pub const fn name(self) -> &'static str { match self { $(Self::$variant => $name),+ } }
            /// The API name shown in Video settings on this platform.
            pub const fn label(self, platform: Platform) -> &'static str {
                if matches!((self, platform), (Self::Gl, Platform::Android)) { return "GLES"; }
                match self { $(Self::$variant => $label),+ }
            }
        }
    };
}
backends!(Vulkan => ("vulkan", "Vulkan"), Dx12 => ("dx12", "DX12"), Gl => ("gl", "GL"), Metal => ("metal", "Metal"));

impl Backend {
    /// Parses a probe's single backend, rejecting lists and unsupported spellings.
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|backend| backend.name() == value)
    }
}

/// Platforms with a native graphics-backend policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Windows,
    Linux,
    Android,
    Macos,
}

impl Platform {
    /// The current client platform; other Unix targets follow Linux's available APIs.
    pub const fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "android") {
            Self::Android
        } else if cfg!(target_os = "macos") {
            Self::Macos
        } else {
            Self::Linux
        }
    }

    /// The single source for available APIs, Video choices and default ordering.
    pub const fn backends(self) -> &'static [Backend] {
        match self {
            Self::Windows => &[Backend::Vulkan, Backend::Dx12],
            Self::Linux | Self::Android => &[Backend::Vulkan, Backend::Gl],
            Self::Macos => &[Backend::Metal],
        }
    }

    /// Inventory uses DX12 on Windows so a broken Vulkan ICD cannot hide the selected GPU.
    pub const fn inventory_order(self) -> &'static [Backend] {
        match self {
            Self::Windows => &[Backend::Dx12, Backend::Vulkan],
            _ => self.backends(),
        }
    }
}

/// Adapter identity and driver details recorded before device creation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Adapter {
    pub name: String,
    pub vendor: u32,
    pub device: u32,
    pub driver: String,
    pub driver_info: String,
    #[serde(default)]
    pub driver_version: String,
    #[serde(default)]
    pub integrated: bool,
}

/// Cache identity also retains the name because GL may report a zero device identifier.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct AdapterKey {
    pub name: String,
    pub vendor: u32,
    pub device: u32,
    pub driver: String,
    pub driver_info: String,
    #[serde(default)]
    pub driver_version: String,
}

impl Adapter {
    /// Includes both wgpu driver strings because backends expose versions in either field.
    pub fn key(&self) -> AdapterKey {
        AdapterKey {
            name: self.name.clone(),
            vendor: self.vendor,
            device: self.device,
            driver: self.driver.clone(),
            driver_info: self.driver_info.clone(),
            driver_version: self.driver_version.clone(),
        }
    }
}

/// Inventory identifies the adapter returned by the same high-performance request as startup.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Inventory {
    pub selected: Adapter,
    pub adapters: Vec<Adapter>,
}

impl Inventory {
    /// Normalizes enumeration order while retaining the selected adapter and every driver revision.
    pub fn fingerprint(&self) -> Vec<AdapterKey> {
        let mut keys: Vec<_> = self.adapters.iter().map(Adapter::key).collect();
        keys.push(self.selected.key());
        keys.sort();
        keys.dedup();
        keys.insert(0, self.selected.key());
        keys
    }
}

/// Chooses by the adapter that will render, never the first enumerated GPU.
pub fn default_order(platform: Platform, selected: &Adapter) -> Vec<Backend> {
    let mut order = platform.backends().to_vec();
    if platform == Platform::Windows && selected.vendor == INTEL_PCI_VENDOR && selected.integrated {
        order.sort_by_key(|backend| *backend != Backend::Dx12);
    }
    order
}

/// A completed probe has a durable verdict even when it crashed or timed out.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Passed,
    Failed,
    TimedOut,
}

/// A running process is undecided before the deadline; only exit zero passes.
pub fn probe_verdict(exit_success: Option<bool>, elapsed: Duration) -> Option<Verdict> {
    match exit_success {
        Some(true) => Some(Verdict::Passed),
        Some(false) => Some(Verdict::Failed),
        None if elapsed >= PROBE_TIMEOUT => Some(Verdict::TimedOut),
        None => None,
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ProbeEntry {
    adapter: AdapterKey,
    backend: Backend,
    verdict: Verdict,
}

/// Driver-specific probe results with bounded invalidation when the adapter set changes.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProbeCache {
    fingerprint: Vec<AdapterKey>,
    entries: Vec<ProbeEntry>,
}

impl ProbeCache {
    /// Whether a previous launch established an adapter inventory.
    pub fn has_inventory(&self) -> bool {
        !self.fingerprint.is_empty()
    }

    /// Invalidates all verdicts after an adapter or driver change, independent of enumeration order.
    pub fn refresh(&mut self, fingerprint: Vec<AdapterKey>) -> bool {
        if self.fingerprint == fingerprint {
            return false;
        }
        self.fingerprint = fingerprint;
        self.entries.clear();
        true
    }

    /// Returns an existing verdict so callers never probe an already tested driver.
    pub fn get(&self, adapter: &AdapterKey, backend: Backend) -> Option<Verdict> {
        self.entries
            .iter()
            .find(|entry| entry.adapter == *adapter && entry.backend == backend)
            .map(|entry| entry.verdict)
    }

    /// Replaces the verdict for one adapter and API without accumulating duplicate entries.
    pub fn record(&mut self, adapter: AdapterKey, backend: Backend, verdict: Verdict) {
        self.entries
            .retain(|entry| entry.adapter != adapter || entry.backend != backend);
        self.entries.push(ProbeEntry {
            adapter,
            backend,
            verdict,
        });
    }
}

/// Pure precedence: environment, Video override, cached verdict, vendor default, sentinel fallback.
/// Explicit choices bypass automatic safety filters. Automatic choices omit blocklisted and failed
/// probes, prefer a passed probe in vendor order, then apply the sentinel as the final backstop.
pub fn candidates(
    defaults: &[Backend],
    supported: &[Backend],
    environment: Option<&[Backend]>,
    video: Option<Backend>,
    verdicts: &[(Backend, Verdict)],
    blocked: &[Backend],
    sentinel_failed: &[Backend],
) -> Vec<Backend> {
    if let Some(explicit) = environment {
        return explicit
            .iter()
            .copied()
            .filter(|backend| supported.contains(backend))
            .collect();
    }
    if let Some(backend) = video {
        return supported
            .contains(&backend)
            .then_some(backend)
            .into_iter()
            .collect();
    }
    let mut order: Vec<_> = defaults
        .iter()
        .copied()
        .filter(|backend| {
            supported.contains(backend)
                && !blocked.contains(backend)
                && !matches!(
                    verdicts
                        .iter()
                        .find(|(api, _)| api == backend)
                        .map(|(_, verdict)| *verdict),
                    Some(Verdict::Failed | Verdict::TimedOut)
                )
        })
        .collect();
    order.sort_by_key(|backend| {
        verdicts
            .iter()
            .find(|(api, _)| api == backend)
            .map(|(_, verdict)| *verdict)
            != Some(Verdict::Passed)
    });
    order.retain(|backend| !sentinel_failed.contains(backend));
    order
}
