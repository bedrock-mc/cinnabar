use std::hash::Hasher;
use std::{collections::BTreeMap, hash::DefaultHasher, sync::Arc};

use super::{ActorRigGeometry, ActorRigVertex};

#[derive(Default)]
pub(super) struct VertexPages {
    pub(super) pages: Vec<Arc<[ActorRigVertex]>>,
    pub(super) fingerprints: Vec<u64>,
    by_content: BTreeMap<(usize, u64), Vec<usize>>,
    // Canonical pages and aliases keep every indexed allocation alive.
    by_pointer: BTreeMap<(usize, usize), usize>,
    /// Equal-content allocations already matched to a page, so repeats skip the byte compare.
    aliases: Vec<Arc<[ActorRigVertex]>>,
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

    /// Hashes select candidates; only exact vertex bytes can share GPU storage. A geometry
    /// whose allocation already matched a page adopts that page without another compare.
    pub(super) fn intern(&mut self, geometry: &mut ActorRigGeometry) -> usize {
        let vertices = Arc::clone(&geometry.vertices);
        let pointer = (vertices.as_ptr() as usize, vertices.len());
        if let Some(&page) = self.by_pointer.get(&pointer) {
            geometry.adopt_vertex_allocation(&self.pages[page]);
            return page;
        }
        let bytes = bytemuck::cast_slice::<ActorRigVertex, u8>(&vertices);
        let fingerprint = geometry.vertex_fingerprint().unwrap_or_else(|| {
            let _span =
                tracing::info_span!("actor.geometry_fingerprint", bytes = bytes.len()).entered();
            #[cfg(test)]
            super::tests::record_vertex_hash(bytes.len());
            let mut hasher = DefaultHasher::new();
            hasher.write(bytes);
            hasher.finish()
        });
        let matched = self
            .by_content
            .get(&(vertices.len(), fingerprint))
            .and_then(|candidates| {
                let _span = tracing::info_span!(
                    "actor.geometry_compare",
                    bytes = bytes.len(),
                    candidates = candidates.len()
                )
                .entered();
                candidates.iter().copied().find(|&page| {
                    #[cfg(test)]
                    super::tests::record_vertex_compare(bytes.len());
                    geometry.share_vertex_allocation(&self.pages[page])
                })
            });
        match matched {
            Some(page) => {
                self.by_pointer.insert(pointer, page);
                self.aliases.push(vertices);
                page
            }
            None => self.push(vertices, fingerprint),
        }
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
