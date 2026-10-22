//! APK-bundled translation for the Linux dedicated server and its cached guest library.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
};

use launcher::local_worlds::world::SetupState;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::{
    Cancel, create_private_dir,
    download::bounded,
    provision::{Platform, Provisioner, RuntimeInfo},
    world::{WorldError, failed},
};

const LEGACY_SOURCE_ARCHIVE: &str = "source.zip";
/// Adds write, SIMD and regular barriers for the server's x86 memory ordering.
const STRONG_MEMORY: &str = "3";

/// Executable names and the translated runtime's pinned guest library.
#[derive(Deserialize)]
pub(super) struct AndroidRuntime {
    pub local_server_library: String,
    pub bds_translation: TranslationPin,
}

#[derive(Clone, Deserialize)]
pub(super) struct TranslationPin {
    pub helper_library: String,
    revision: String,
    guest_library: String,
    guest_base_url: String,
    guest_directory: String,
    guest_sha256: String,
    guest_bytes: u64,
    native_runtime: NativeRuntime,
}

/// Native Linux libraries installed as executable APK libraries alongside the translator.
#[derive(Clone, Deserialize)]
struct NativeRuntime {
    libraries: Vec<NativeLibrary>,
}

#[derive(Clone, Deserialize)]
struct NativeLibrary {
    packaged: String,
    #[serde(default)]
    loader: bool,
}

/// The manifest shared with the Android packager.
pub(super) fn android_runtime() -> &'static AndroidRuntime {
    static PIN: OnceLock<AndroidRuntime> = OnceLock::new();
    PIN.get_or_init(|| {
        serde_json::from_str(include_str!("../../../../packaging/android/runtime.json"))
            .expect("valid Android runtime manifest")
    })
}

impl TranslationPin {
    /// The single guest library at the same pinned upstream revision as the translator.
    fn library_url(&self) -> String {
        format!(
            "{}{}/{}/{}",
            self.guest_base_url, self.revision, self.guest_directory, self.guest_library
        )
    }

    /// Accepts only the exact guest library identified by the manifest's size and digest.
    fn verify_library(&self, bytes: &[u8]) -> Result<(), WorldError> {
        if bytes.len() as u64 != self.guest_bytes
            || format!("{:x}", Sha256::digest(bytes)) != self.guest_sha256
        {
            return Err(failed(
                "translated server runtime library failed verification",
            ));
        }
        Ok(())
    }
}

/// One translator helper plus its private, versioned guest-library cache.
pub(crate) struct Translation {
    helper: PathBuf,
    cache: PathBuf,
    pin: TranslationPin,
}

impl Translation {
    /// Detects a bundled Android helper without invoking any desktop runtime.
    pub(super) fn detect(platform: Platform, core: &Path, cache: PathBuf) -> Option<Self> {
        if platform.os != "android" || platform.arch != "arm64" {
            return None;
        }
        let helper = core.with_file_name(&android_runtime().bds_translation.helper_library);
        let pin = &android_runtime().bds_translation;
        (helper.is_file()
            && pin
                .native_runtime
                .libraries
                .iter()
                .all(|library| helper.with_file_name(&library.packaged).is_file()))
        .then(|| Self {
            helper,
            cache,
            pin: android_runtime().bds_translation.clone(),
        })
    }

    /// A complete packaged runtime makes BDS available as the default backend.
    pub(super) fn runtime(&self) -> RuntimeInfo {
        RuntimeInfo::translated()
    }

    /// Configures translation without changing the server's stdin, output or working directory.
    pub(super) fn command(
        &self,
        server: &Path,
        provisioner: &Provisioner,
        cancel: &Cancel,
    ) -> Result<Command, WorldError> {
        let pin = &self.pin;
        let directory = self.cache.join(&pin.revision);
        create_private_dir(&directory).map_err(|error| failed(error.to_string()))?;
        self.ensure_library(pin, &directory, provisioner, cancel)
            .map_err(|error| {
                provisioner.fail_with("translated server runtime setup failed", error)
            })?;
        let install = super::provision::install_dir(server)?;
        let libraries = std::env::join_paths([install, directory.as_path()])
            .map_err(|error| failed(format!("translated runtime library path: {error}")))?;
        let native_dir = self
            .helper
            .parent()
            .ok_or_else(|| failed("missing runtime directory"))?;
        let native = &pin.native_runtime.libraries;
        let loader = native
            .iter()
            .find(|library| library.loader)
            .ok_or_else(|| failed("missing translated runtime loader"))?;
        let preloads = std::env::join_paths(
            native
                .iter()
                .filter(|library| !library.loader)
                .map(|library| native_dir.join(&library.packaged)),
        )
        .map_err(|error| failed(format!("native runtime library path: {error}")))?;
        let mut command = Command::new(native_dir.join(&loader.packaged));
        command
            .arg("--inhibit-cache")
            .arg("--library-path")
            .arg(native_dir)
            .arg("--preload")
            .arg(preloads)
            .arg(&self.helper)
            .arg(server)
            .env_remove("LD_PRELOAD")
            .env_remove("LD_AUDIT")
            .env("BOX64_LD_LIBRARY_PATH", libraries)
            .env("BOX64_DYNAREC_STRONGMEM", STRONG_MEMORY);
        Ok(command)
    }

    /// Reuses a verified guest library, downloading only its pinned bytes when missing or damaged.
    fn ensure_library(
        &self,
        pin: &TranslationPin,
        directory: &Path,
        provisioner: &Provisioner,
        cancel: &Cancel,
    ) -> Result<(), WorldError> {
        if cancel.is_cancelled() {
            return Err(WorldError::Cancelled);
        }
        let library = directory.join(&pin.guest_library);
        let valid =
            if fs::metadata(&library).is_ok_and(|metadata| metadata.len() == pin.guest_bytes) {
                let bytes = fs::read(&library).map_err(|error| failed(error.to_string()))?;
                pin.verify_library(&bytes).is_ok()
            } else {
                false
            };
        if !valid {
            provisioner.set_op(Some(SetupState::Downloading), None, 0, pin.guest_bytes);
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| failed(error.to_string()))?;
            let bytes = runtime.block_on(fetch_library(pin, provisioner, cancel))?;
            pin.verify_library(&bytes)?;
            publish(&library, &bytes, cancel)?;
        }
        // Older installs kept the whole source archive. Drop it only after the library is valid.
        let _ = fs::remove_file(directory.join(LEGACY_SOURCE_ARCHIVE));
        provisioner.set_op(None, None, 0, 0);
        Ok(())
    }
}

/// Downloads only the pinned guest library, checking cancellation, size and stalls between chunks.
async fn fetch_library(
    pin: &TranslationPin,
    provisioner: &Provisioner,
    cancel: &Cancel,
) -> Result<Vec<u8>, WorldError> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(super::download::AGENT)
        .build()
        .map_err(|error| failed(error.to_string()))?;
    let mut response = bounded(
        client.get(pin.library_url()).send(),
        provisioner.stall_timeout,
        cancel,
    )
    .await
    .map_err(super::download::Fetch::into_error)?
    .and_then(reqwest::Response::error_for_status)
    .map_err(|error| failed(format!("download translated runtime: {error}")))?;
    let mut bytes = Vec::new();
    while let Some(chunk) = bounded(response.chunk(), provisioner.stall_timeout, cancel)
        .await
        .map_err(super::download::Fetch::into_error)?
        .map_err(|error| failed(format!("read translated runtime download: {error}")))?
    {
        if chunk.len() as u64 > pin.guest_bytes.saturating_sub(bytes.len() as u64) {
            return Err(failed("translated runtime library exceeds its pinned size"));
        }
        bytes.extend_from_slice(&chunk);
        provisioner.set_op(
            Some(SetupState::Downloading),
            None,
            bytes.len() as u64,
            pin.guest_bytes,
        );
    }
    Ok(bytes)
}

/// Atomically replaces a runtime cache file, preserving the old one on error or cancellation.
fn publish(path: &Path, bytes: &[u8], cancel: &Cancel) -> Result<(), WorldError> {
    let parent = path
        .parent()
        .ok_or_else(|| failed("runtime cache has no parent"))?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|error| failed(error.to_string()))?;
    temporary
        .write_all(bytes)
        .map_err(|error| failed(error.to_string()))?;
    if cancel.is_cancelled() {
        return Err(WorldError::Cancelled);
    }
    temporary
        .persist(path)
        .map_err(|error| failed(error.to_string()))?;
    Ok(())
}

#[cfg(test)]
#[path = "translated_tests.rs"]
mod tests;
