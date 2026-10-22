//! APK-bundled translation for the Linux dedicated server and its cached guest library.

use std::{
    fs,
    io::{Cursor, Read, Write},
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

const SOURCE_ARCHIVE: &str = "source.zip";
/// Adds write, SIMD and regular barriers for the server's x86 memory ordering.
const STRONG_MEMORY: &str = "3";

/// Executable names and the translated runtime's immutable source archive.
#[derive(Deserialize)]
pub(super) struct AndroidRuntime {
    pub local_server_library: String,
    pub bds_translation: TranslationPin,
}

#[derive(Clone, Deserialize)]
pub(super) struct TranslationPin {
    pub helper_library: String,
    archive_base_url: String,
    revision: String,
    archive_sha256: String,
    archive_bytes: u64,
    guest_library: String,
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
    /// The pinned upstream source archive; guest code is downloaded, never bundled.
    fn archive_url(&self) -> String {
        format!("{}{}", self.archive_base_url, self.revision)
    }

    /// Validates the complete archive before reading its one required guest library.
    fn guest_bytes(&self, archive: &[u8]) -> Result<Vec<u8>, WorldError> {
        if archive.len() as u64 != self.archive_bytes
            || format!("{:x}", Sha256::digest(archive)) != self.archive_sha256
        {
            return Err(failed(
                "translated server runtime archive failed verification",
            ));
        }
        let mut archive = zip::ZipArchive::new(Cursor::new(archive))
            .map_err(|error| failed(format!("read translated runtime archive: {error}")))?;
        let entry = format!("box64-{}/x64lib/{}", self.revision, self.guest_library);
        let mut library = Vec::new();
        archive
            .by_name(&entry)
            .map_err(|error| failed(format!("locate translated runtime library: {error}")))?
            .read_to_end(&mut library)
            .map_err(|error| failed(format!("read translated runtime library: {error}")))?;
        Ok(library)
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

    /// The backend is available for explicit selection; Dragonfly remains the default.
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

    /// Revalidates cached guest code, acquiring the source archive only when necessary.
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
        let source = directory.join(SOURCE_ARCHIVE);
        let cached =
            if fs::metadata(&source).is_ok_and(|metadata| metadata.len() == pin.archive_bytes) {
                Some(fs::read(&source).map_err(|error| failed(error.to_string()))?)
                    .filter(|bytes| format!("{:x}", Sha256::digest(bytes)) == pin.archive_sha256)
            } else {
                None
            };
        let archive = match cached {
            Some(bytes) => bytes,
            None => {
                provisioner.set_op(Some(SetupState::Downloading), None, 0, 0);
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|error| failed(error.to_string()))?;
                let bytes = runtime.block_on(fetch_archive(pin, provisioner, cancel))?;
                pin.guest_bytes(&bytes)?;
                publish(&source, &bytes, cancel)?;
                bytes
            }
        };
        let library = pin.guest_bytes(&archive)?;
        publish(&directory.join(&pin.guest_library), &library, cancel)?;
        provisioner.set_op(None, None, 0, 0);
        Ok(())
    }
}

/// Downloads only the pinned HTTPS archive, checking cancellation and stalls between chunks.
async fn fetch_archive(
    pin: &TranslationPin,
    provisioner: &Provisioner,
    cancel: &Cancel,
) -> Result<Vec<u8>, WorldError> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| failed(error.to_string()))?;
    let mut response = bounded(
        client.get(pin.archive_url()).send(),
        provisioner.stall_timeout,
        cancel,
    )
    .await
    .map_err(super::download::Fetch::into_error)?
    .and_then(reqwest::Response::error_for_status)
    .map_err(|error| failed(format!("download translated runtime: {error}")))?;
    let total = response.content_length().unwrap_or(0);
    let mut bytes = Vec::new();
    while let Some(chunk) = bounded(response.chunk(), provisioner.stall_timeout, cancel)
        .await
        .map_err(super::download::Fetch::into_error)?
        .map_err(|error| failed(format!("read translated runtime download: {error}")))?
    {
        if chunk.len() as u64 > pin.archive_bytes.saturating_sub(bytes.len() as u64) {
            return Err(failed("translated runtime archive exceeds its pinned size"));
        }
        bytes.extend_from_slice(&chunk);
        provisioner.set_op(
            Some(SetupState::Downloading),
            None,
            bytes.len() as u64,
            total,
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
