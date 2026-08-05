use std::{
    fs::{self, File, OpenOptions},
    io,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use tracing::{debug, warn};

const CACHE_MAX_AGE: Duration = Duration::from_secs(30 * 24 * 60 * 60);

pub(crate) struct CachedResourcePack {
    pub(crate) bytes: Box<[u8]>,
    pub(crate) content_key: Box<str>,
}

pub(crate) struct ResourcePackCache {
    dir: PathBuf,
}

impl ResourcePackCache {
    pub(crate) fn open(dir: &Path) -> Option<Self> {
        if dir.as_os_str().is_empty() {
            return None;
        }
        if let Err(error) = fs::create_dir_all(dir) {
            warn!(path = %dir.display(), %error, "resource-pack cache is unavailable");
            return None;
        }
        Some(Self {
            dir: dir.to_path_buf(),
        })
    }

    pub(crate) fn load(
        &self,
        uuid: &str,
        version: &str,
        advertised_content_key: &str,
    ) -> Option<CachedResourcePack> {
        let base = cache_base(uuid, version)?;
        let pack_path = self.dir.join(format!("{base}.zip"));
        let bytes = match fs::read(&pack_path) {
            Ok(bytes) if !bytes.is_empty() => bytes,
            Ok(_) => {
                warn!(path = %pack_path.display(), "ignoring empty cached resource pack");
                return None;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => return None,
            Err(error) => {
                warn!(path = %pack_path.display(), %error, "failed to read cached resource pack");
                return None;
            }
        };

        let key_path = self.dir.join(format!("{base}.key"));
        let content_key = fs::read(&key_path)
            .ok()
            .filter(|key| !key.is_empty())
            .and_then(|key| String::from_utf8(key).ok())
            .unwrap_or_else(|| advertised_content_key.to_owned());

        if let Err(error) = self.touch_last_used(&base) {
            warn!(path = %pack_path.display(), %error, "failed to touch cached resource pack");
        }
        debug!(pack = %base, "resource-pack cache hit");

        Some(CachedResourcePack {
            bytes: bytes.into_boxed_slice(),
            content_key: content_key.into_boxed_str(),
        })
    }

    pub(crate) fn write(
        &self,
        uuid: &str,
        version: &str,
        bytes: &[u8],
        content_key: &str,
    ) -> io::Result<()> {
        let Some(base) = cache_base(uuid, version) else {
            return Ok(());
        };
        let pack_path = self.dir.join(format!("{base}.zip"));
        fs::write(&pack_path, bytes)?;

        let key_path = self.dir.join(format!("{base}.key"));
        if content_key.is_empty() {
            remove_if_present(&key_path)?;
        } else {
            fs::write(&key_path, content_key.as_bytes())?;
        }

        self.touch_last_used(&base)
    }

    pub(crate) fn cleanup_old_packs(&self, uuid: &str, current_version: &str) -> io::Result<()> {
        let Some(current_base) = cache_base(uuid, current_version) else {
            return Ok(());
        };
        let current_pack_name = format!("{current_base}.zip");
        let current_key_name = format!("{current_base}.key");
        let id_prefix = format!("{uuid}_");
        let cutoff = SystemTime::now()
            .checked_sub(CACHE_MAX_AGE)
            .unwrap_or(SystemTime::UNIX_EPOCH);

        for entry in fs::read_dir(&self.dir)? {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    warn!(path = %self.dir.display(), %error, "failed to inspect resource-pack cache entry");
                    continue;
                }
            };
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(error) => {
                    warn!(path = %entry.path().display(), %error, "failed to inspect resource-pack cache entry type");
                    continue;
                }
            };
            if file_type.is_dir() {
                continue;
            }

            let name = entry.file_name();
            let name = name.to_string_lossy();
            let path = entry.path();

            if let Some(base) = name.strip_suffix(".lastused") {
                let modified = match entry.metadata().and_then(|metadata| metadata.modified()) {
                    Ok(modified) => modified,
                    Err(error) => {
                        warn!(path = %path.display(), %error, "failed to read resource-pack cache marker time");
                        continue;
                    }
                };
                if modified < cutoff {
                    remove_if_present(&self.dir.join(format!("{base}.zip")))?;
                    remove_if_present(&self.dir.join(format!("{base}.key")))?;
                    remove_if_present(&path)?;
                    debug!(pack = base, "evicted unused resource pack");
                }
                continue;
            }

            if !name.starts_with(&id_prefix)
                || name == current_pack_name
                || name == current_key_name
            {
                continue;
            }
            remove_if_present(&path)?;
            debug!(pack = %name, uuid, current_version, "removed old resource-pack version");
        }
        Ok(())
    }

    fn touch_last_used(&self, base: &str) -> io::Result<()> {
        let marker = self.dir.join(format!("{base}.lastused"));
        let file = OpenOptions::new().create(true).write(true).open(marker)?;
        touch_file(&file)
    }
}

fn cache_base(uuid: &str, version: &str) -> Option<String> {
    if is_safe_component(uuid) && is_safe_component(version) {
        Some(format!("{uuid}_{version}"))
    } else {
        warn!(
            uuid,
            version, "skipping resource-pack cache for unsafe identifier"
        );
        None
    }
}

fn is_safe_component(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn touch_file(file: &File) -> io::Result<()> {
    file.set_modified(SystemTime::now())
}
