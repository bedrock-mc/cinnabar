use zeroize::Zeroize;

/// Maximum packs accepted from one local negotiation.
pub const MAX_RESOURCE_PACKS: usize = 32;
/// Maximum compressed bytes retained for one pack.
pub const MAX_RESOURCE_PACK_BYTES: u64 = 64 * 1024 * 1024;
/// Maximum compressed bytes retained across the selected stack.
pub const MAX_RESOURCE_PACK_TOTAL_BYTES: u64 = 128 * 1024 * 1024;
/// Maximum server-selected chunk size.
pub const MAX_RESOURCE_PACK_CHUNK_BYTES: u32 = 1024 * 1024;
/// Maximum chunks accepted for one pack.
pub const MAX_RESOURCE_PACK_CHUNKS: u32 = 4096;

/// The offer identity a stored archive answers to: what ResourcePacksInfo advertises per pack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourcePackIdentity<'a> {
    pub pack_id: uuid::Uuid,
    pub version: &'a str,
    pub size: u64,
}

/// Keeps digest-verified server-pack archives across joins so negotiation can decline packs the
/// client already holds. Archives are stored as received; content keys are never persisted.
pub trait ResourcePackStore: Send + Sync + std::fmt::Debug {
    /// Returns the archive stored under `identity`; a damaged entry is a miss.
    fn load(&self, identity: ResourcePackIdentity<'_>) -> Option<Vec<u8>>;
    fn store(&self, identity: ResourcePackIdentity<'_>, archive: &[u8]);
}

/// A secret retained only for the lifetime of an in-memory archive handoff.
pub struct ResourcePackContentKey(Vec<u8>);

impl ResourcePackContentKey {
    pub(crate) fn from_string(mut value: String) -> Self {
        let bytes = value.as_bytes().to_vec();
        value.zeroize();
        Self(bytes)
    }

    /// Borrows the content key without formatting or copying it.
    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}

impl Drop for ResourcePackContentKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

/// One verified compressed archive in exact server-selected application order.
/// This type intentionally does not implement `Debug` so content keys cannot be
/// emitted accidentally by diagnostic formatting.
pub struct ResourcePackArchive {
    pub pack_id: uuid::Uuid,
    pub version: String,
    pub sub_pack_name: String,
    pub archive: Vec<u8>,
    pub content_key: ResourcePackContentKey,
}

impl ResourcePackArchive {
    /// Builds a locally sourced archive that has no content-encryption key.
    pub fn unencrypted(
        pack_id: uuid::Uuid,
        version: String,
        sub_pack_name: String,
        archive: Vec<u8>,
    ) -> Self {
        Self {
            pack_id,
            version,
            sub_pack_name,
            archive,
            content_key: ResourcePackContentKey(Vec::new()),
        }
    }

    /// Builds an archive from a locally cached pack and its content key.
    pub fn with_content_key(
        pack_id: uuid::Uuid,
        version: String,
        sub_pack_name: String,
        archive: Vec<u8>,
        key: Vec<u8>,
    ) -> Self {
        Self {
            pack_id,
            version,
            sub_pack_name,
            archive,
            content_key: ResourcePackContentKey(key),
        }
    }
}

/// One-shot login handoff. Archives are captured but never parsed or applied.
#[derive(Default)]
pub struct ResourcePackHandoff {
    archives: Vec<ResourcePackArchive>,
    required: bool,
}

impl ResourcePackHandoff {
    pub(crate) fn new(archives: Vec<ResourcePackArchive>) -> Self {
        Self::from_archives(archives)
    }

    /// Builds a one-shot handoff from already captured archive carriers.
    pub fn from_archives(archives: Vec<ResourcePackArchive>) -> Self {
        Self {
            archives,
            required: false,
        }
    }

    /// Marks the packs as server-required, so a client that cannot apply one must refuse the join.
    pub fn with_required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    /// Whether the offer or the stack required these packs.
    pub fn required(&self) -> bool {
        self.required
    }

    pub fn is_empty(&self) -> bool {
        self.archives.is_empty()
    }

    pub fn len(&self) -> usize {
        self.archives.len()
    }

    pub fn into_archives(mut self) -> Vec<ResourcePackArchive> {
        std::mem::take(&mut self.archives)
    }
}
