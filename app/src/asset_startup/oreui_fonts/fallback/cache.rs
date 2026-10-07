//! Transactional admission keeps unsupported and failed scalars out of the raster cache.

use std::collections::BTreeSet;

const MAX_COMPILE_ATTEMPTS: usize = 8;

pub(super) struct GlyphCache {
    characters: Vec<char>,
    pinned: usize,
    maximum: usize,
}

pub(super) struct Update<T> {
    pub(super) font: Option<T>,
    pub(super) evicted: Vec<char>,
    pub(super) error: Option<String>,
}

impl GlyphCache {
    pub(super) fn new(characters: Vec<char>, maximum: usize) -> Self {
        Self {
            pinned: characters.len(),
            characters,
            maximum,
        }
    }

    pub(super) fn update<T>(
        &mut self,
        pending: Vec<char>,
        mut compile: impl FnMut(&[char]) -> Result<(T, Vec<char>), String>,
    ) -> Update<T> {
        let previous = self.characters.clone();
        let known: BTreeSet<_> = previous.iter().copied().collect();
        let mut supported_seen = known.clone();
        let mut unique = BTreeSet::new();
        let pending: Vec<_> = pending
            .into_iter()
            .filter(|ch| !known.contains(ch) && unique.insert(*ch))
            .collect();
        let mut batches = vec![pending];
        let mut attempts = 0;
        let mut update = Update {
            font: None,
            evicted: Vec::new(),
            error: None,
        };
        while let Some(additions) = batches.pop() {
            if attempts == MAX_COMPILE_ATTEMPTS {
                break;
            }
            let mut proposed = self.characters.clone();
            proposed.extend(additions.iter().copied());
            attempts += 1;
            let result = compile(&proposed).and_then(|(font, supported)| {
                let supported: BTreeSet<_> = supported.into_iter().collect();
                if self.characters[..self.pinned]
                    .iter()
                    .any(|ch| !supported.contains(ch))
                {
                    return Err("fallback replacement omitted pinned glyphs".into());
                }
                proposed.retain(|ch| supported.contains(ch));
                supported_seen.extend(proposed.iter().copied());
                let overflow = proposed.len().saturating_sub(self.maximum);
                if overflow == 0 {
                    return Ok((font, proposed));
                }
                if attempts == MAX_COMPILE_ATTEMPTS || self.pinned + overflow > proposed.len() {
                    return Err("fallback replacement exceeds admission budget".into());
                }
                proposed.drain(self.pinned..self.pinned + overflow);
                attempts += 1;
                compile(&proposed).and_then(|(font, supported)| {
                    let supported: BTreeSet<_> = supported.into_iter().collect();
                    if self.characters[..self.pinned]
                        .iter()
                        .any(|ch| !supported.contains(ch))
                    {
                        return Err("fallback replacement omitted pinned glyphs".into());
                    }
                    proposed.retain(|ch| supported.contains(ch));
                    Ok((font, proposed))
                })
            });
            match result {
                Ok((font, characters)) => {
                    self.characters = characters;
                    update.font = Some(font);
                }
                Err(reason) => {
                    update.error = Some(reason);
                    if additions.len() > 1 {
                        let middle = additions.len() / 2;
                        batches.push(additions[middle..].to_vec());
                        batches.push(additions[..middle].to_vec());
                    }
                }
            }
        }
        let admitted: BTreeSet<_> = self.characters.iter().copied().collect();
        update.evicted = supported_seen
            .into_iter()
            .filter(|ch| !admitted.contains(ch))
            .collect();
        update
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn supported(characters: &[char]) -> Result<(Vec<char>, Vec<char>), String> {
        let supported: Vec<_> = characters.iter().copied().filter(|ch| *ch != '?').collect();
        Ok((supported.clone(), supported))
    }

    #[test]
    fn unsupported_requests_never_enter_the_cache_or_evict_valid_glyphs() {
        let mut cache = GlyphCache::new(vec!['a'], 3);
        cache.update(vec!['b', 'c'], supported);
        let update = cache.update(vec!['?', '?'], supported);
        assert_eq!(cache.characters, vec!['a', 'b', 'c']);
        assert!(update.evicted.is_empty());
        let update = cache.update(vec!['d'], supported);
        assert_eq!(cache.characters, vec!['a', 'c', 'd']);
        assert_eq!(update.evicted, vec!['b']);
    }

    #[test]
    fn mixed_failures_salvage_valid_additions_without_poisoning_later_batches() {
        let mut cache = GlyphCache::new(vec!['a'], 4);
        let compile = |characters: &[char]| {
            if characters.contains(&'!') {
                Err("invalid glyph".into())
            } else {
                supported(characters)
            }
        };
        let update = cache.update(vec!['b', '!', 'c'], compile);
        assert!(update.error.is_some());
        assert_eq!(update.font, Some(vec!['a', 'b', 'c']));
        assert_eq!(cache.characters, vec!['a', 'b', 'c']);
        let update = cache.update(vec!['d'], compile);
        assert_eq!(update.font, Some(vec!['a', 'b', 'c', 'd']));
        assert!(update.error.is_none());
    }

    #[test]
    fn successfully_rasterized_new_glyphs_trimmed_from_the_cache_can_return() {
        let mut cache = GlyphCache::new(vec!['a'], 2);
        let update = cache.update(vec!['b', 'c', '?', 'd'], supported);
        assert_eq!(cache.characters, vec!['a', 'd']);
        assert_eq!(update.evicted, vec!['b', 'c']);
        assert!(!update.evicted.contains(&'?'));
        let update = cache.update(vec!['b'], supported);
        assert_eq!(cache.characters, vec!['a', 'b']);
        assert_eq!(update.font, Some(vec!['a', 'b']));
        assert_eq!(update.evicted, vec!['d']);
    }

    #[test]
    fn failed_replacements_keep_the_valid_cache_and_do_not_release_its_deduplication() {
        let mut cache = GlyphCache::new(vec!['a'], 2);
        cache.update(vec!['b'], supported);
        let mut attempts = 0;
        let update: Update<()> = cache.update(('c'..='z').collect(), |_| {
            attempts += 1;
            Err("invalid atlas".into())
        });
        assert_eq!(cache.characters, vec!['a', 'b']);
        assert!(update.font.is_none());
        assert!(update.evicted.is_empty());
        assert!(attempts <= MAX_COMPILE_ATTEMPTS);
    }
}
