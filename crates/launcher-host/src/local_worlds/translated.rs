//! APK-bundled translation for the Linux dedicated server and its cached guest library.

use std::{
    fs, io,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
};

use launcher::local_worlds::world::SetupState;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::{
    Cancel,
    code_cache::{self, CodeCache},
    create_private_dir,
    download::bounded,
    provision::{Platform, Provisioner, RuntimeInfo},
    unpack::install_digest,
    world::{WorldError, failed},
};

const LEGACY_SOURCE_ARCHIVE: &str = "source.zip";
/// Adds write, SIMD and regular barriers for the server's x86 memory ordering.
const STRONG_MEMORY: &str = "3";
/// The translator's reusable code cache, beside the versioned guest-library folders.
pub(super) const CODE_CACHE_DIR: &str = "code-cache";

/// The certificate bundle shared by Android bootstrap and the translated server command.
pub(super) fn trust_path(runtime_dir: &Path) -> PathBuf {
    runtime_dir.join("platform-trust.pem")
}

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
    /// Identifies the pinned native runtime package the libraries come from.
    sha256: String,
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

#[cfg(test)]
impl TranslationPin {
    /// A pin of the guest `library` under `base_url`, with a fixture native runtime of a loader
    /// named `loader` and one preloaded `libnative.so`.
    pub(super) fn fixture(library: &[u8], base_url: String) -> Self {
        Self {
            helper_library: "helper".to_owned(),
            guest_base_url: base_url,
            guest_directory: "x64lib".to_owned(),
            revision: "fixture".to_owned(),
            guest_sha256: format!("{:x}", Sha256::digest(library)),
            guest_bytes: library.len() as u64,
            guest_library: "libguest.so".to_owned(),
            native_runtime: NativeRuntime {
                sha256: "native runtime fixture".to_owned(),
                libraries: vec![
                    NativeLibrary {
                        packaged: "loader".to_owned(),
                        loader: true,
                    },
                    NativeLibrary {
                        packaged: "libnative.so".to_owned(),
                        loader: false,
                    },
                ],
            },
        }
    }

    /// The cached guest library's folder and file name beneath a runtime folder.
    #[cfg(unix)]
    pub(super) fn guest_path(&self, runtime_dir: &Path) -> PathBuf {
        runtime_dir.join(&self.revision).join(&self.guest_library)
    }
}

/// One translator helper plus its private, versioned guest-library and translated-code caches.
pub(crate) struct Translation {
    helper: PathBuf,
    cache: PathBuf,
    pin: TranslationPin,
    /// The helper's digest, hashed once per process; `None` when it could not be read.
    helper_digest: OnceLock<Option<String>>,
}

impl Translation {
    /// A translation through `helper`, keeping its caches under `cache`; the helper is hashed on
    /// first use.
    pub(super) fn new(helper: PathBuf, cache: PathBuf, pin: TranslationPin) -> Self {
        Self {
            helper,
            cache,
            pin,
            helper_digest: OnceLock::new(),
        }
    }

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
        .then(|| Self::new(helper, cache, android_runtime().bds_translation.clone()))
    }

    /// A complete packaged runtime makes BDS available as the default backend.
    pub(super) fn runtime(&self) -> RuntimeInfo {
        RuntimeInfo::translated()
    }

    /// Configures translation without changing the server's stdin, output or working directory.
    /// The returned cache use must be finished with the launched server's ending.
    pub(super) fn command(
        &self,
        server: &Path,
        provisioner: &Provisioner,
        cancel: &Cancel,
    ) -> Result<(Command, code_cache::Use), WorldError> {
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
            .env("BOX64_DYNAREC_STRONGMEM", STRONG_MEMORY)
            // Settings come only from this command, never from a host or home rc file.
            .env("BOX64_NORCFILES", "1");
        command.env_remove("SSL_CERT_FILE");
        let trust = trust_path(&self.cache);
        if fs::metadata(&trust).is_ok_and(|metadata| metadata.is_file() && metadata.len() != 0) {
            command.env("SSL_CERT_FILE", trust);
        }
        let used = self.code_cache(server).map_or_else(
            || CodeCache::off(code_cache::Off::Unidentified),
            |cache| cache.prepare(code_cache::available_bytes),
        );
        used.apply(&mut command);
        Ok((command, used))
    }

    /// The code cache for the installed binary `server` and the packaged runtime; `None` when
    /// either identity is unknown. The install's recorded archive digest identifies the server:
    /// the translator itself checks each cached file against the mapped binary's path and length,
    /// and each block against its guest code, so the binary always runs from its install path.
    pub(super) fn code_cache(&self, server: &Path) -> Option<CodeCache> {
        let executable = server.file_name()?.to_str()?;
        let digest = install_digest(super::provision::install_dir(server).ok()?)?;
        let helper = self.helper_digest()?;
        let pin = &self.pin;
        Some(CodeCache::new(
            self.cache.join(CODE_CACHE_DIR),
            executable,
            &[
                &digest,
                helper,
                &pin.native_runtime.sha256,
                &pin.guest_sha256,
                STRONG_MEMORY,
                code_cache::UNCOMPRESSED,
            ],
        ))
    }

    /// The private folder holding this translation's guest library, code cache and trust store.
    pub(super) fn runtime_dir(&self) -> &Path {
        &self.cache
    }

    /// Whether the pinned guest library is already cached with its pinned size and digest. Only
    /// reads; [`Self::command`] downloads a missing or damaged library.
    pub(super) fn library_ready(&self) -> bool {
        let pin = &self.pin;
        let library = self.cache.join(&pin.revision).join(&pin.guest_library);
        fs::metadata(&library).is_ok_and(|metadata| metadata.len() == pin.guest_bytes)
            && fs::read(&library).is_ok_and(|bytes| pin.verify_library(&bytes).is_ok())
    }

    /// The packaged helper's digest, so any change to its bytes starts a new cache generation.
    fn helper_digest(&self) -> Option<&str> {
        self.helper_digest
            .get_or_init(|| {
                file_sha256(&self.helper)
                    .inspect_err(|error| {
                        tracing::warn!("hash translator for its code cache: {error}");
                    })
                    .ok()
            })
            .as_deref()
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

/// The SHA-256 of the file at `path` as lowercase hex, read in fixed-size chunks.
fn file_sha256(path: &Path) -> io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0; 64 << 10];
    loop {
        match file.read(&mut buffer) {
            Ok(0) => return Ok(format!("{:x}", hash.finalize())),
            Ok(read) => hash.update(&buffer[..read]),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
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
