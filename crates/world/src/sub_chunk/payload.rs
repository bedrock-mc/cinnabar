use super::{BlockIds, Reader, SubChunk};
use sha2::{Digest, Sha256};
use std::{
    any::Any,
    sync::{Arc, Mutex},
};

#[derive(Clone)]
pub(super) struct InlinePayload {
    length: usize,
    hash: [u8; 32],
    y_index: Option<i8>,
    context: Arc<dyn Any + Send + Sync>,
}

/// A cache update never changes block values or the section's shared Arc identity.
#[derive(Debug, Default)]
pub(super) struct InlinePayloadCache(Mutex<Option<Arc<InlinePayload>>>);

impl Clone for InlinePayloadCache {
    /// Clones the identity into an independent cache so mutations cannot invalidate older snapshots.
    fn clone(&self) -> Self {
        Self(Mutex::new(self.snapshot()))
    }
}

impl InlinePayloadCache {
    /// Holds the lock only long enough to clone the immutable identity.
    fn snapshot(&self) -> Option<Arc<InlinePayload>> {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()
    }
    /// Publishes an identity for unchanged block values without retaining the wire bytes.
    fn replace(&self, payload: Option<Arc<InlinePayload>>) {
        *self.0.lock().unwrap_or_else(|poison| poison.into_inner()) = payload;
    }
    /// Clears the identity before an exclusively owned section is modified.
    pub(super) fn clear(&mut self) {
        *self
            .0
            .get_mut()
            .unwrap_or_else(|poison| poison.into_inner()) = None;
    }
}

impl std::fmt::Debug for InlinePayload {
    /// Shows cache size without dumping the retained registry snapshot.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("InlinePayload")
            .field("length", &self.length)
            .finish_non_exhaustive()
    }
}

impl SubChunk {
    /// Skips exactly the bytes consumed by an earlier complete decode with matching registries.
    /// Truncated decodes have no cache entry because appended bytes could change their result.
    /// The cached wire Y must fit the slot even if an equivalent encoding kept an older header.
    pub(crate) fn reuse_inline(
        &self,
        reader: &mut Reader<'_>,
        ids: &dyn BlockIds,
        slot_y: i32,
    ) -> bool {
        let Some(payload) = self.inline_payload.snapshot() else {
            return false;
        };
        if payload
            .y_index
            .is_some_and(|actual| i32::from(actual) != slot_y)
            || !ids.inline_cache_context_matches(payload.context.as_ref())
        {
            return false;
        }
        let Some(bytes) = reader.remaining().get(..payload.length) else {
            return false;
        };
        if <[u8; 32]>::from(Sha256::digest(bytes)) != payload.hash {
            return false;
        }
        reader.read_exact(payload.length);
        true
    }

    /// Transfers a freshly decoded wire identity when equal block values keep the resident Arc.
    pub(crate) fn retain_inline_identity_from(&self, decoded: &Self) {
        self.inline_payload
            .replace(decoded.inline_payload.snapshot());
    }

    /// Retains the consumed byte length, digest, wire Y, and registry identity after a complete decode.
    pub(crate) fn read_inline(
        reader: &mut Reader<'_>,
        ids: &dyn BlockIds,
        context: Option<&Arc<dyn Any + Send + Sync>>,
    ) -> Self {
        let bytes = reader.remaining();
        let start = reader.position();
        let section = Self::read(reader, ids);
        if !reader.failed_read
            && !section.has_no_storages()
            && let Some(context) = context
        {
            let length = reader.position() - start;
            section.inline_payload.replace(Some(Arc::new(InlinePayload {
                length,
                hash: Sha256::digest(&bytes[..length]).into(),
                y_index: section.y_index(),
                context: Arc::clone(context),
            })));
        }
        section
    }
}
