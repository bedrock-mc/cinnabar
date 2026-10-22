use assets::SessionEntityPack;
use resource_pack::LayeredPackView;
use resource_pack::StackFingerprint;
use std::{path::PathBuf, sync::Arc};

#[path = "pixel_reuse.rs"]
mod pixel_reuse;

/// Pack content, reference definitions, and the startup artwork paths a compile reads.
#[derive(Clone)]
pub(super) struct EntityInputs {
    pub(super) stack: StackFingerprint,
    pub(super) vanilla: Option<Arc<assets::VanillaEntityRefs>>,
    pub(super) vanilla_pack_dir: Option<PathBuf>,
    pub(super) equipment: Option<Arc<assets::RuntimeEquipmentCatalog>>,
}

impl PartialEq for EntityInputs {
    /// Loaded texture-path identities keep rebuilt artwork tables out of stale cache entries.
    fn eq(&self, other: &Self) -> bool {
        let Self {
            stack,
            vanilla,
            vanilla_pack_dir,
            equipment,
        } = self;
        stack == &other.stack
            && vanilla == &other.vanilla
            && vanilla_pack_dir == &other.vanilla_pack_dir
            && equipment
                .as_ref()
                .map(|catalog| catalog.texture_paths_sha256())
                == other
                    .equipment
                    .as_ref()
                    .map(|catalog| catalog.texture_paths_sha256())
    }
}

type CachedEntities = (
    EntityInputs,
    Option<Arc<SessionEntityPack>>,
    Option<std::collections::BTreeSet<resource_pack::PackDependency>>,
);

/// A compiled pack and, when asked for, the entity blob it was built from.
pub(super) type Compiled = (Option<Arc<SessionEntityPack>>, Option<Box<[u8]>>);

/// Runs the encode and write of a fresh compile's disk entry.
type StoreEntry = fn(Box<dyn FnOnce() + Send>);

/// The join and composed reload compiles, retained across reconnects.
pub(super) struct EntityCache {
    last: std::sync::Mutex<std::collections::VecDeque<CachedEntities>>,
    store: StoreEntry,
}

impl Default for EntityCache {
    /// Writes disk entries before a compile returns.
    fn default() -> Self {
        Self {
            last: std::sync::Mutex::new(std::collections::VecDeque::new()),
            store: |write| write(),
        }
    }
}

/// Writes optional disk-cache entries on one worker, with room for one waiting entry.
/// Overflow or incomplete writes leave a cache miss; joins never wait for disk encoding.
fn store_in_background(write: Box<dyn FnOnce() + Send>) {
    type Writes = std::sync::Mutex<std::sync::mpsc::SyncSender<Box<dyn FnOnce() + Send>>>;
    static WRITER: std::sync::OnceLock<Option<Writes>> = std::sync::OnceLock::new();
    let writer = WRITER.get_or_init(|| {
        let (writes, pending) = std::sync::mpsc::sync_channel::<Box<dyn FnOnce() + Send>>(1);
        std::thread::Builder::new()
            .name("entity-pack-store".to_owned())
            .spawn(move || pending.into_iter().for_each(|write| write()))
            .map_err(|error| tracing::debug!(%error, "compiled entity packs are not stored"))
            .ok()
            .map(|_| std::sync::Mutex::new(writes))
    });
    let Some(writer) = writer else {
        return;
    };
    let queued = writer
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .try_send(write);
    if queued.is_err() {
        tracing::debug!("a compiled entity pack was not stored while others were");
    }
}

impl EntityCache {
    /// Returns the cached pack for `inputs`, then a disk hit, compiling without holding the lock.
    /// `None` once `cancelled` stopped the compile, which then records and stores nothing.
    pub(super) fn get_or_compile(
        &self,
        inputs: EntityInputs,
        view: &LayeredPackView,
        disk: Option<&crate::compile_cache::CompileCache>,
        cancelled: &dyn Fn() -> bool,
        compile: impl FnOnce(
            &LayeredPackView,
            Option<&assets::VanillaEntityRefs>,
            Option<&std::path::Path>,
            Option<&assets::RuntimeEquipmentCatalog>,
            bool,
            &dyn Fn() -> bool,
        ) -> Option<Compiled>,
    ) -> Option<Option<Arc<SessionEntityPack>>> {
        {
            let mut cache = self.lock();
            if let Some(index) = cache.iter().position(|(cached, _, files)| {
                *cached == inputs && (view.dependencies().is_none() || files.is_some())
            }) {
                let entry = cache.remove(index).expect("a matched entry is present");
                if let (Some(dependencies), Some(files)) = (view.dependencies(), &entry.2) {
                    dependencies.extend(files.clone());
                }
                let pack = entry.1.clone();
                cache.push_front(entry);
                return Some(pack);
            }
        }
        // Only a tracked compile knows its reads, which a reload diff of a disk hit needs.
        let disk = disk.filter(|_| view.dependencies().is_some());
        let key = disk.map(|_| disk_key(&inputs));
        let stored = disk.zip(key.as_ref()).and_then(|(disk, key)| {
            let entry = disk.load(key)?;
            let decoded = decode_entry(&entry);
            if decoded.is_none() {
                tracing::warn!("discarding an undecodable compiled entity pack");
            }
            decoded
        });
        let pack = if let Some((pack, files)) = stored {
            if let Some(dependencies) = view.dependencies() {
                dependencies.extend(files);
            }
            self.reuse_pixels(pack)
        } else {
            let (pack, blob) = compile(
                view,
                inputs.vanilla.as_deref(),
                inputs.vanilla_pack_dir.as_deref(),
                inputs.equipment.as_deref(),
                disk.is_some(),
                cancelled,
            )?;
            let pack = self.reuse_pixels(pack);
            if let (Some(disk), Some(key), Some(files)) = (disk, key, view.dependencies()) {
                let (disk, files, stored) = (disk.clone(), files.snapshot(), pack.clone());
                (self.store)(Box::new(move || {
                    if let Some(entry) = encode_entry(&files, stored.as_deref(), blob.as_deref()) {
                        disk.store(&key, &entry);
                    }
                }));
            }
            pack
        };
        let mut cache = self.lock();
        cache.retain(|(cached, _, _)| *cached != inputs);
        cache.push_front((
            inputs,
            pack.clone(),
            view.dependencies().map(|files| files.snapshot()),
        ));
        cache.truncate(2);
        Some(pack)
    }

    /// Shares unchanged rasters before another owner or disk writer retains the new catalog.
    fn reuse_pixels(&self, next: Option<Arc<SessionEntityPack>>) -> Option<Arc<SessionEntityPack>> {
        let previous = self.lock().front().and_then(|(_, pack, _)| pack.clone());
        next.map(|next| {
            let next = match previous {
                Some(previous) => pixel_reuse::reuse(&previous, next),
                None => next,
            };
            next.with_shared_equipment_pixels()
        })
    }

    /// Recovers the bounded cache after a panicking reader.
    fn lock(&self) -> std::sync::MutexGuard<'_, std::collections::VecDeque<CachedEntities>> {
        self.last
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Names the stack, reference definitions, loaded equipment texture paths, and the build.
fn disk_key(inputs: &EntityInputs) -> [u8; 32] {
    use crate::compile_cache::{cache_key, part};
    use sha2::Digest;
    let mut key = cache_key("entities");
    for (id, version, subpack, content) in &inputs.stack {
        for bytes in [
            id.as_bytes(),
            version.as_bytes(),
            subpack.as_bytes(),
            content,
        ] {
            part(&mut key, bytes);
        }
    }
    part(&mut key, &vanilla_digest(inputs.vanilla.as_ref()));
    part(&mut key, &[u8::from(inputs.equipment.is_some())]);
    part(
        &mut key,
        &inputs
            .equipment
            .as_ref()
            .map(|catalog| catalog.texture_paths_sha256())
            .unwrap_or_default(),
    );
    if let Some(path) = &inputs.vanilla_pack_dir {
        part(&mut key, path.as_os_str().as_encoded_bytes());
    }
    key.finalize().into()
}

/// Digest of the refs' canonical JSON, computed once per installed refs.
fn vanilla_digest(refs: Option<&Arc<assets::VanillaEntityRefs>>) -> [u8; 32] {
    use sha2::Digest;
    type Memo = Option<(Arc<assets::VanillaEntityRefs>, [u8; 32])>;
    static MEMO: std::sync::Mutex<Memo> = std::sync::Mutex::new(None);
    let Some(refs) = refs else {
        return [0; 32];
    };
    let mut memo = MEMO
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((memoized, digest)) = memo.as_ref()
        && Arc::ptr_eq(memoized, refs)
    {
        return *digest;
    }
    let digest = sha2::Sha256::digest(refs.to_json()).into();
    *memo = Some((Arc::clone(refs), digest));
    digest
}

/// Reads first, then a presence flag and the pack; `None` when the stack defines no entity.
fn encode_entry(
    files: &std::collections::BTreeSet<resource_pack::PackDependency>,
    pack: Option<&SessionEntityPack>,
    blob: Option<&[u8]>,
) -> Option<Vec<u8>> {
    let mut entry = Vec::new();
    resource_pack::PackDependency::encode_set(files, &mut entry);
    match pack {
        None => entry.push(0),
        Some(pack) => {
            entry.push(1);
            match pack.encode(blob?) {
                Ok(bytes) => entry.extend_from_slice(&bytes),
                Err(error) => {
                    tracing::debug!(%error, "compiled entity pack is not cacheable");
                    return None;
                }
            }
        }
    }
    Some(entry)
}

fn decode_entry(
    entry: &[u8],
) -> Option<(
    Option<Arc<SessionEntityPack>>,
    std::collections::BTreeSet<resource_pack::PackDependency>,
)> {
    let (files, rest) = resource_pack::PackDependency::decode_set(entry)?;
    let pack = match rest.split_first()? {
        (0, []) => None,
        (1, bytes) => Some(Arc::new(SessionEntityPack::decode(bytes).ok()?)),
        _ => return None,
    };
    Some((pack, files))
}

pub(super) static ENTITY_CACHE: EntityCache = EntityCache {
    last: std::sync::Mutex::new(std::collections::VecDeque::new()),
    store: store_in_background,
};

#[cfg(test)]
#[path = "cache_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "reload_tests.rs"]
mod reload_tests;
