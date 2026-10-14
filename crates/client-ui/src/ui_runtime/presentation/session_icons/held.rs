//! Bounded vertex storage for admitted session sprites used by player previews.

use std::{collections::BTreeMap, sync::Arc};

use assets::texture::policy::MAX_SUBSCRIBER_BYTES;
use render_model::ActorRigVertex;

use super::IconKey;

#[derive(Default)]
pub(super) struct HeldMeshes {
    entries: BTreeMap<IconKey, Entry>,
    bytes: usize,
    sequence: u64,
}

struct Entry {
    vertices: Arc<[ActorRigVertex]>,
    used: u64,
}

impl HeldMeshes {
    /// Reuses a resident source without allocating, recording only its latest request order.
    pub(super) fn get(&mut self, key: IconKey) -> Option<Arc<[ActorRigVertex]>> {
        let entry = self.entries.get_mut(&key)?;
        self.sequence = self.sequence.wrapping_add(1);
        entry.used = self.sequence;
        Some(Arc::clone(&entry.vertices))
    }

    /// Retains one source within the shared storage limit, retiring least-recently-used meshes.
    pub(super) fn insert(&mut self, key: IconKey, vertices: Arc<[ActorRigVertex]>) -> Option<()> {
        let bytes = std::mem::size_of_val(&*vertices);
        if bytes > MAX_SUBSCRIBER_BYTES {
            return None;
        }
        if self.bytes > MAX_SUBSCRIBER_BYTES - bytes {
            let mut oldest = self
                .entries
                .iter()
                .map(|(key, entry)| (entry.used, *key))
                .collect::<Vec<_>>();
            oldest.sort_unstable();
            for (_, key) in oldest {
                let retired = self.entries.remove(&key)?;
                self.bytes -= std::mem::size_of_val(&*retired.vertices);
                if self.bytes <= MAX_SUBSCRIBER_BYTES - bytes {
                    break;
                }
            }
        }
        self.sequence = self.sequence.wrapping_add(1);
        self.entries.insert(
            key,
            Entry {
                vertices,
                used: self.sequence,
            },
        );
        self.bytes += bytes;
        Some(())
    }
}
