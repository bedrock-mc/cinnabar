use crate::StackFingerprint;
use assets::SessionEntityPack;
use resource_pack::LayeredPackView;
use std::{path::PathBuf, sync::Arc};

/// Everything a compile reads: the pack stack and the vanilla refs it resolved against.
#[derive(Clone, PartialEq)]
pub(super) struct EntityInputs {
    pub(super) stack: StackFingerprint,
    pub(super) vanilla: Option<Arc<assets::VanillaEntityRefs>>,
    pub(super) vanilla_pack_dir: Option<PathBuf>,
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

/// The previous compile, reused when the same inputs rejoin.
pub(super) struct EntityCache {
    last: std::sync::Mutex<Option<CachedEntities>>,
    store: StoreEntry,
}

impl Default for EntityCache {
    /// Writes disk entries before a compile returns.
    fn default() -> Self {
        Self {
            last: std::sync::Mutex::new(None),
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
            bool,
            &dyn Fn() -> bool,
        ) -> Option<Compiled>,
    ) -> Option<Option<Arc<SessionEntityPack>>> {
        {
            let cache = self.lock();
            if let Some((cached, pack, files)) = cache.as_ref()
                && *cached == inputs
                && (view.dependencies().is_none() || files.is_some())
            {
                if let (Some(dependencies), Some(files)) = (view.dependencies(), files) {
                    dependencies.extend(files.clone());
                }
                return Some(pack.clone());
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
            pack
        } else {
            let (pack, blob) = compile(
                view,
                inputs.vanilla.as_deref(),
                inputs.vanilla_pack_dir.as_deref(),
                disk.is_some(),
                cancelled,
            )?;
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
        *self.lock() = Some((
            inputs,
            pack.clone(),
            view.dependencies().map(|files| files.snapshot()),
        ));
        Some(pack)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<CachedEntities>> {
        self.last
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Names the stack, the vanilla refs it resolves against, and the build.
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
    last: std::sync::Mutex::new(None),
    store: store_in_background,
};

#[cfg(test)]
#[path = "cache_tests.rs"]
mod tests;
