use std::{
    collections::BTreeMap,
    hash::{DefaultHasher, Hasher},
    sync::Arc,
};

use super::{ActorRigGeometry, ActorRigVertex};

#[derive(Default)]
pub(super) struct VertexPages {
    pub(super) pages: Vec<Arc<[ActorRigVertex]>>,
    pub(super) fingerprints: Vec<u64>,
    by_content: BTreeMap<(usize, u64), Vec<usize>>,
    // Canonical pages keep every indexed allocation alive.
    by_pointer: BTreeMap<(usize, usize), usize>,
}

impl VertexPages {
    /// Indexes existing immutable pages without rehashing their contents.
    pub(super) fn retained(pages: &[Arc<[ActorRigVertex]>], fingerprints: &[u64]) -> Self {
        let mut retained = Self::default();
        for (page, &fingerprint) in pages.iter().zip(fingerprints) {
            retained.push(Arc::clone(page), fingerprint);
        }
        retained
    }

    /// Hashes select candidates; only exact vertex bytes can share GPU storage.
    pub(super) fn intern(&mut self, geometry: &mut ActorRigGeometry) -> usize {
        let vertices = Arc::clone(&geometry.vertices);
        let pointer = (vertices.as_ptr() as usize, vertices.len());
        if let Some(&page) = self.by_pointer.get(&pointer) {
            return page;
        }
        let bytes = bytemuck::cast_slice::<ActorRigVertex, u8>(&vertices);
        let fingerprint = geometry.vertex_fingerprint().unwrap_or_else(|| {
            let _span =
                bevy::log::info_span!("actor.geometry_fingerprint", bytes = bytes.len()).entered();
            #[cfg(test)]
            super::tests::record_vertex_hash(bytes.len());
            let mut hasher = DefaultHasher::new();
            hasher.write(bytes);
            hasher.finish()
        });
        if let Some(candidates) = self.by_content.get(&(vertices.len(), fingerprint)) {
            let _span = bevy::log::info_span!(
                "actor.geometry_compare",
                bytes = bytes.len(),
                candidates = candidates.len()
            )
            .entered();
            for &page in candidates {
                if geometry.share_vertex_allocation(&self.pages[page]) {
                    return page;
                }
            }
        }
        self.push(Arc::clone(&vertices), fingerprint)
    }

    /// Records one canonical allocation and its prepared fingerprint.
    fn push(&mut self, vertices: Arc<[ActorRigVertex]>, fingerprint: u64) -> usize {
        let page = self.pages.len();
        self.by_pointer
            .insert((vertices.as_ptr() as usize, vertices.len()), page);
        self.by_content
            .entry((vertices.len(), fingerprint))
            .or_default()
            .push(page);
        self.pages.push(vertices);
        self.fingerprints.push(fingerprint);
        page
    }
}
