use super::*;

impl ChunkStore {
    /// Shares collision indexes until block authority or a loaded palette changes.
    pub fn collision_snapshot(&self) -> Arc<Self> {
        let mut cached = self
            .collision_snapshot
            .lock()
            .expect("collision snapshot lock");
        if let Some(snapshot) = cached.upgrade() {
            return snapshot;
        }
        let snapshot = Arc::new(Self {
            chunks: self
                .chunks
                .iter()
                .map(|(&key, chunk)| {
                    (
                        key,
                        Chunk {
                            sub_chunks: Arc::clone(&chunk.sub_chunks),
                            ..Chunk::default()
                        },
                    )
                })
                .collect(),
            loaded_chunks: self.loaded_chunks.clone(),
            authoritative_sub_chunks: self.authoritative_sub_chunks.clone(),
            collision_revisions: self.collision_revisions.clone(),
            collision_revision_allocator: Arc::clone(&self.collision_revision_allocator),
            collision_snapshot: Mutex::default(),
        });
        *cached = Arc::downgrade(&snapshot);
        snapshot
    }

    /// Leaves historical snapshots owned only by their prediction frames.
    pub(super) fn invalidate_collision_snapshot(&mut self) {
        *self
            .collision_snapshot
            .get_mut()
            .expect("collision snapshot lock") = Weak::new();
    }
}

#[cfg(test)]
mod tests;
