use std::{
    collections::BTreeMap,
    hash::{DefaultHasher, Hasher},
    sync::Arc,
};

use super::ActorRigVertex;

#[derive(Default)]
pub(super) struct VertexPages {
    pub(super) pages: Vec<Arc<[ActorRigVertex]>>,
    pub(super) fingerprints: Vec<u64>,
    by_content: BTreeMap<(usize, u64), Vec<usize>>,
    by_pointer: BTreeMap<(usize, usize), PinnedPage>,
}

/// The source allocation is held so its address key can't be reused by another slice.
type PinnedPage = (Arc<[ActorRigVertex]>, usize);

impl VertexPages {
    pub(super) fn retained(pages: &[Arc<[ActorRigVertex]>], fingerprints: &[u64]) -> Self {
        let mut retained = Self::default();
        for (page, &fingerprint) in pages.iter().zip(fingerprints) {
            retained.push(Arc::clone(page), fingerprint);
        }
        retained
    }

    /// Hashes select candidates; only exact vertex bytes can share GPU storage.
    pub(super) fn intern(&mut self, vertices: &Arc<[ActorRigVertex]>) -> usize {
        let pointer = (vertices.as_ptr() as usize, vertices.len());
        if let Some((_, page)) = self.by_pointer.get(&pointer) {
            return *page;
        }
        let bytes = bytemuck::cast_slice::<ActorRigVertex, u8>(vertices);
        let mut hasher = DefaultHasher::new();
        hasher.write(bytes);
        let fingerprint = hasher.finish();
        if let Some(candidates) = self.by_content.get(&(vertices.len(), fingerprint)) {
            for &page in candidates {
                if bytes == bytemuck::cast_slice::<ActorRigVertex, u8>(&self.pages[page]) {
                    self.by_pointer
                        .insert(pointer, (Arc::clone(vertices), page));
                    return page;
                }
            }
        }
        self.push(Arc::clone(vertices), fingerprint)
    }

    fn push(&mut self, vertices: Arc<[ActorRigVertex]>, fingerprint: u64) -> usize {
        let page = self.pages.len();
        self.by_pointer.insert(
            (vertices.as_ptr() as usize, vertices.len()),
            (Arc::clone(&vertices), page),
        );
        self.by_content
            .entry((vertices.len(), fingerprint))
            .or_default()
            .push(page);
        self.pages.push(vertices);
        self.fingerprints.push(fingerprint);
        page
    }
}
