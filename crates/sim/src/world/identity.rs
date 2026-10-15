//! Construction and merging of canonical world collision identities.

use std::{cmp::Ordering, collections::BTreeMap};

use world::{ChunkCollisionRevision, ChunkKey};

use super::{
    CollisionRegistryIdentity, MAX_COLLISION_IDENTITY_CHUNKS, WorldCollisionIdentity,
    WorldQueryError,
};

impl WorldCollisionIdentity {
    /// Canonicalizes revisions into ascending distinct columns, polling `chunks` one at a
    /// time and stopping at the first conflict or the first column past the limit.
    pub fn new(
        registry: CollisionRegistryIdentity,
        chunks: impl IntoIterator<Item = ChunkCollisionRevision>,
    ) -> Result<Self, WorldQueryError> {
        let mut chunks = chunks.into_iter();
        // While input stays ascending, every column above the last is new and one equal
        // to it is a repeat, so the sorted list itself answers what the map would.
        let mut sorted: Vec<ChunkCollisionRevision> = Vec::new();
        let mut unsorted = None;
        for revision in chunks.by_ref() {
            match sorted.last() {
                Some(last) if last.chunk == revision.chunk => {
                    if last.revision != revision.revision {
                        return Err(WorldQueryError::ChunkRevisionConflict {
                            chunk: revision.chunk,
                        });
                    }
                }
                Some(last) if revision.chunk < last.chunk => {
                    unsorted = Some(revision);
                    break;
                }
                _ => {
                    if sorted.len() == MAX_COLLISION_IDENTITY_CHUNKS {
                        return Err(limit_exceeded());
                    }
                    sorted.push(revision);
                }
            }
        }
        let Some(first_unsorted) = unsorted else {
            return Ok(Self {
                registry,
                chunks: sorted.into_boxed_slice(),
            });
        };
        let mut by_chunk: BTreeMap<ChunkKey, ChunkCollisionRevision> = sorted
            .into_iter()
            .map(|revision| (revision.chunk, revision))
            .collect();
        for revision in std::iter::once(first_unsorted).chain(chunks) {
            if let Some(previous) = by_chunk.get(&revision.chunk) {
                if previous.revision != revision.revision {
                    return Err(WorldQueryError::ChunkRevisionConflict {
                        chunk: revision.chunk,
                    });
                }
                continue;
            }
            if by_chunk.len() == MAX_COLLISION_IDENTITY_CHUNKS {
                return Err(limit_exceeded());
            }
            by_chunk.insert(revision.chunk, revision);
        }
        Ok(Self {
            registry,
            chunks: by_chunk.into_values().collect(),
        })
    }

    /// The identity of one column, equal to [`Self::new`] over that single revision.
    pub(super) fn single(
        registry: CollisionRegistryIdentity,
        revision: ChunkCollisionRevision,
    ) -> Self {
        Self {
            registry,
            chunks: Box::new([revision]),
        }
    }

    /// Unions two identities exactly as [`Self::new`] over `self`'s columns followed by
    /// `other`'s, without rebuilding an ordered map when both are already canonical.
    pub fn merge(&self, other: &Self) -> Result<Self, WorldQueryError> {
        if self.registry != other.registry {
            return Err(WorldQueryError::RegistryIdentityMismatch);
        }
        if !is_canonical(&self.chunks) || !is_canonical(&other.chunks) {
            return Self::new(
                self.registry,
                self.chunks.iter().chain(other.chunks.iter()).copied(),
            );
        }
        // As `new` would: `self` alone may exceed the limit, then each of `other`'s columns,
        // in ascending order, either matches one of `self`'s or adds one more column.
        if self.chunks.len() > MAX_COLLISION_IDENTITY_CHUNKS {
            return Err(limit_exceeded());
        }
        let mut count = self.chunks.len();
        let mut cursor = 0;
        for revision in other.chunks.iter() {
            while cursor < self.chunks.len() && self.chunks[cursor].chunk < revision.chunk {
                cursor += 1;
            }
            match self.chunks.get(cursor) {
                Some(existing) if existing.chunk == revision.chunk => {
                    if existing.revision != revision.revision {
                        return Err(WorldQueryError::ChunkRevisionConflict {
                            chunk: revision.chunk,
                        });
                    }
                }
                _ if count == MAX_COLLISION_IDENTITY_CHUNKS => return Err(limit_exceeded()),
                _ => count += 1,
            }
        }
        if count == self.chunks.len() {
            return Ok(self.clone());
        }
        let mut chunks: Vec<ChunkCollisionRevision> = Vec::with_capacity(count);
        let mut left = self.chunks.iter().copied().peekable();
        let mut right = other.chunks.iter().copied().peekable();
        while let (Some(a), Some(b)) = (left.peek(), right.peek()) {
            match a.chunk.cmp(&b.chunk) {
                Ordering::Less => chunks.extend(left.next()),
                Ordering::Greater => chunks.extend(right.next()),
                Ordering::Equal => {
                    chunks.extend(left.next());
                    right.next();
                }
            }
        }
        chunks.extend(left);
        chunks.extend(right);
        Ok(Self {
            registry: self.registry,
            chunks: chunks.into_boxed_slice(),
        })
    }
}

/// The error for an identity spanning more than [`MAX_COLLISION_IDENTITY_CHUNKS`] columns.
fn limit_exceeded() -> WorldQueryError {
    WorldQueryError::IdentityChunkLimitExceeded {
        max: MAX_COLLISION_IDENTITY_CHUNKS,
    }
}

/// Whether revisions are in strictly ascending column order, the form
/// [`WorldCollisionIdentity::new`] produces.
fn is_canonical(chunks: &[ChunkCollisionRevision]) -> bool {
    chunks.windows(2).all(|pair| pair[0].chunk < pair[1].chunk)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> CollisionRegistryIdentity {
        CollisionRegistryIdentity {
            protocol: 1001,
            id_space: super::super::CollisionIdSpace::Sequential,
            preg_sha256: [0; 32],
        }
    }

    fn revision(x: i32, revision: u64) -> ChunkCollisionRevision {
        ChunkCollisionRevision {
            chunk: ChunkKey::new(0, x, 0),
            revision,
        }
    }

    /// Reference union: the ordered-map construction over both column lists.
    fn reference(
        left: &WorldCollisionIdentity,
        right: &WorldCollisionIdentity,
    ) -> Result<WorldCollisionIdentity, WorldQueryError> {
        let mut by_chunk: BTreeMap<ChunkKey, ChunkCollisionRevision> = BTreeMap::new();
        for revision in left.chunks.iter().chain(right.chunks.iter()) {
            if let Some(previous) = by_chunk.get(&revision.chunk) {
                if previous.revision != revision.revision {
                    return Err(WorldQueryError::ChunkRevisionConflict {
                        chunk: revision.chunk,
                    });
                }
                continue;
            }
            if by_chunk.len() == MAX_COLLISION_IDENTITY_CHUNKS {
                return Err(limit_exceeded());
            }
            by_chunk.insert(revision.chunk, *revision);
        }
        Ok(WorldCollisionIdentity {
            registry: left.registry,
            chunks: by_chunk.into_values().collect(),
        })
    }

    #[test]
    fn canonical_merge_matches_the_ordered_map_union() {
        let identity = |xs: &[i32]| {
            WorldCollisionIdentity::new(registry(), xs.iter().map(|&x| revision(x, 1))).unwrap()
        };
        let cases: [(&[i32], &[i32]); 6] = [
            (&[], &[]),
            (&[1], &[1]),
            (&[1, 3], &[2]),
            (&[2], &[1, 3, 5]),
            (&[-4, 0, 9], &[-4, 9]),
            (&[], &[7, 8]),
        ];
        for (left, right) in cases {
            let (left, right) = (identity(left), identity(right));
            assert_eq!(left.merge(&right), reference(&left, &right));
        }
    }

    #[test]
    fn merge_reports_conflicts_and_limits_like_the_ordered_map_union() {
        let left =
            WorldCollisionIdentity::new(registry(), [revision(1, 1), revision(2, 1)]).unwrap();
        let right = WorldCollisionIdentity::new(registry(), [revision(2, 5)]).unwrap();
        assert_eq!(left.merge(&right), reference(&left, &right));
        assert!(left.merge(&right).is_err());

        let max = i32::try_from(MAX_COLLISION_IDENTITY_CHUNKS).unwrap();
        let full =
            WorldCollisionIdentity::new(registry(), (0..max).map(|x| revision(x, 1))).unwrap();
        let extra = WorldCollisionIdentity::new(
            registry(),
            [revision(-1, 1), revision(0, 2), revision(max, 1)],
        )
        .unwrap();
        assert_eq!(full.merge(&extra), reference(&full, &extra));
        assert!(full.merge(&full).is_ok());
    }

    #[test]
    fn unordered_columns_still_canonicalize() {
        let identity = WorldCollisionIdentity::new(
            registry(),
            [revision(3, 1), revision(1, 1), revision(3, 1)],
        )
        .unwrap();
        assert_eq!(identity.chunks.as_ref(), &[revision(1, 1), revision(3, 1)]);
        assert_eq!(
            WorldCollisionIdentity::new(registry(), [revision(3, 1), revision(3, 2)]),
            Err(WorldQueryError::ChunkRevisionConflict {
                chunk: ChunkKey::new(0, 3, 0),
            })
        );
    }
}
