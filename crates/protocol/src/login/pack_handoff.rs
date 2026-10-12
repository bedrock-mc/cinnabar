//! The one-shot resource-pack handoff a login captures for the client to apply.

use zeroize::Zeroize;

/// A content-encryption key retained only while its archive is in memory; zeroed on drop.
pub struct ResourcePackContentKey(Vec<u8>);

impl ResourcePackContentKey {
    /// Borrows the key without formatting or copying it.
    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}

impl Drop for ResourcePackContentKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

/// One verified compressed archive in server-selected application order.
/// It has no `Debug`, so diagnostic formatting cannot emit its content key.
pub struct ResourcePackArchive {
    pub pack_id: uuid::Uuid,
    pub version: String,
    pub sub_pack_name: String,
    pub archive: Vec<u8>,
    pub content_key: ResourcePackContentKey,
}

impl ResourcePackArchive {
    /// A locally sourced archive with no content key.
    pub fn unencrypted(
        pack_id: uuid::Uuid,
        version: String,
        sub_pack_name: String,
        archive: Vec<u8>,
    ) -> Self {
        Self::with_content_key(pack_id, version, sub_pack_name, archive, Vec::new())
    }

    /// An archive and the key its content is encrypted with.
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

/// Archives a login captured, in stack order; they are never parsed or applied here.
#[derive(Default)]
pub struct ResourcePackHandoff {
    archives: Vec<ResourcePackArchive>,
    required: bool,
}

impl ResourcePackHandoff {
    /// A handoff of optional packs.
    pub fn from_archives(archives: Vec<ResourcePackArchive>) -> Self {
        Self {
            archives,
            required: false,
        }
    }

    /// Marks the packs server-required, so a client that cannot apply one refuses the join.
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

    pub fn into_archives(self) -> Vec<ResourcePackArchive> {
        self.archives
    }
}
