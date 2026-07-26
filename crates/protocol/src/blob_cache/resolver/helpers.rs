use super::*;

pub(super) fn stable_unique(hashes: &[u64]) -> Vec<u64> {
    let mut seen = HashSet::with_capacity(hashes.len());
    hashes
        .iter()
        .copied()
        .filter(|hash| seen.insert(*hash))
        .collect()
}
