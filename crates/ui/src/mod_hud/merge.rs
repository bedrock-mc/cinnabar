//! Retained load-order merging without copying unchanged component output each frame.

use super::{Hud, MAX_HUD_CARDS, MAX_HUD_CELLS, MAX_HUD_COMPASSES, MAX_HUD_ROWS};
use std::sync::Arc;

#[derive(Default)]
pub struct HudMergeCache {
    revisions: Vec<u64>,
    content: Option<Arc<Hud>>,
}

impl HudMergeCache {
    /// Sources must use process-unique revisions for immutable, already validated HUDs.
    /// The first source owns policies; later sources contribute whole distinct cards within bounds.
    pub fn update<'a>(
        &mut self,
        sources: impl Iterator<Item = (u64, &'a Hud)> + Clone,
    ) -> Option<Arc<Hud>> {
        if self
            .revisions
            .iter()
            .copied()
            .eq(sources.clone().map(|(revision, _)| revision))
        {
            return self.content.clone();
        }
        self.revisions.clear();
        self.revisions
            .extend(sources.clone().map(|(revision, _)| revision));
        let mut sources = sources;
        let Some((_, first)) = sources.next() else {
            self.content = None;
            return None;
        };
        let mut hud = first.clone();
        let mut rows = hud.cards.iter().map(|c| c.rows.len()).sum::<usize>();
        let mut cells = hud.cards.iter().map(|c| c.cells.len()).sum::<usize>();
        let mut compasses = hud.cards.iter().filter(|c| c.compass.is_some()).count();
        for (_, other) in sources {
            for card in &other.cards {
                if !hud.cards.iter().any(|existing| existing.id == card.id)
                    && hud.cards.len() < MAX_HUD_CARDS
                    && rows + card.rows.len() <= MAX_HUD_ROWS
                    && cells + card.cells.len() <= MAX_HUD_CELLS
                    && compasses + usize::from(card.compass.is_some()) <= MAX_HUD_COMPASSES
                {
                    rows += card.rows.len();
                    cells += card.cells.len();
                    compasses += usize::from(card.compass.is_some());
                    hud.cards.push(card.clone());
                }
            }
        }
        self.content = Some(Arc::new(hud));
        self.content.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Equal inputs retain identity; reordering, replacement and removal revoke the cached merge.
    #[test]
    fn versions_fence_content_and_load_order() {
        let a: Hud = serde_json::from_str(
            r#"{"cards":[{"id":"shared","rows":[{"label":"a","value":""}]}]}"#,
        )
        .unwrap();
        let b: Hud = serde_json::from_str(
            r#"{"cards":[{"id":"shared","rows":[{"label":"b","value":""}]}]}"#,
        )
        .unwrap();
        let mut cache = HudMergeCache::default();
        let first = cache.update([(1, &a), (2, &b)].into_iter()).unwrap();
        let same = cache.update([(1, &a), (2, &b)].into_iter()).unwrap();
        assert!(Arc::ptr_eq(&first, &same));
        let reordered = cache.update([(2, &b), (1, &a)].into_iter()).unwrap();
        assert_eq!(reordered.cards[0].rows[0].label, "b");
        assert!(!Arc::ptr_eq(&same, &reordered));
        let replaced = cache.update([(3, &a)].into_iter()).unwrap();
        assert_eq!(replaced.cards[0].rows[0].label, "a");
        assert!(cache.update(std::iter::empty()).is_none());
    }
}
