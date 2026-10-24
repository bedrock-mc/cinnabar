//! Stability policy shared by startup and the persisted Video setting.

use serde::{Deserialize, Serialize};

pub mod blocklist;
#[cfg(test)]
mod tests;

/// Intel's PCI vendor identifier, used to decode its Windows Vulkan driver versions.
pub const INTEL_PCI_VENDOR: u32 = 0x8086;
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
    /// Renderer-supported APIs shared by the Video choices and native candidate discovery.
    pub const AVAILABLE: &'static [Self] = &[Self::Vulkan, Self::Dx12, Self::Metal];

    /// Parses a single backend, rejecting lists and unsupported spellings.
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

    /// The platform subset of available APIs in default order.
    pub const fn backends(self) -> &'static [Backend] {
        match self {
            Self::Windows => &[Backend::Dx12, Backend::Vulkan],
            Self::Linux | Self::Android => &[Backend::Vulkan],
            Self::Macos => &[Backend::Metal],
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

/// Applies environment and Video overrides before platform defaults and crash exclusions.
/// Explicit choices bypass automatic filters, but must still be supported by this platform.
pub fn candidates(
    defaults: &[Backend],
    supported: &[Backend],
    environment: Option<&[Backend]>,
    video: Option<Backend>,
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
    defaults
        .iter()
        .copied()
        .filter(|backend| supported.contains(backend) && !sentinel_failed.contains(backend))
        .collect()
}
