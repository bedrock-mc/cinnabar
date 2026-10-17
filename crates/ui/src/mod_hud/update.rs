//! Bounded retained HUD edits that reuse the existing card and presentation contracts.

use super::{Card, Hud, MAX_HUD_CARDS, MAX_HUD_CELLS, MAX_TEXT_BYTES};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// One transactional HUD edit; omitted presentation retains the previous metadata.
#[derive(Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct HudUpdate {
    pub cards: Vec<Card>,
    pub remove: Vec<String>,
    pub cell_changes: Vec<CellChange>,
    /// Uses the ordinary HUD schema, with an empty cards list; cards are edited separately.
    pub presentation: Option<Hud>,
}

/// Paint or text changes reuse an existing cell's admitted rectangle and card geometry.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CellChange {
    pub card: String,
    pub index: usize,
    pub color: Option<[f32; 4]>,
    pub background: Option<[f32; 4]>,
    pub label: Option<String>,
    pub value: Option<String>,
}

impl HudUpdate {
    /// Builds and validates a replacement without changing the previously committed HUD.
    pub fn apply(self, previous: Option<&Hud>) -> Result<Hud, String> {
        if self.cards.len() > MAX_HUD_CARDS || self.remove.len() > MAX_HUD_CARDS {
            return Err("HUD update exceeds its card limit".into());
        }
        if self.cell_changes.len() > MAX_HUD_CELLS {
            return Err("HUD update exceeds its cell change limit".into());
        }
        let mut identities = HashSet::new();
        for id in self.cards.iter().map(|card| &card.id).chain(&self.remove) {
            if id.is_empty() || id.len() > MAX_TEXT_BYTES || !identities.insert(id) {
                return Err("HUD update has invalid or duplicate card identities".into());
            }
        }
        let mut content = previous.cloned().unwrap_or_default();
        if let Some(mut presentation) = self.presentation {
            if !presentation.cards.is_empty() {
                return Err("HUD update presentation must have no cards".into());
            }
            presentation.cards = std::mem::take(&mut content.cards);
            content = presentation;
        }
        content.cards.retain(|card| !self.remove.contains(&card.id));
        for card in self.cards {
            if let Some(existing) = content
                .cards
                .iter_mut()
                .find(|existing| existing.id == card.id)
            {
                *existing = card;
            } else {
                content.cards.push(card);
            }
        }
        let mut cells = HashSet::new();
        for change in self.cell_changes {
            if !cells.insert((change.card.clone(), change.index)) {
                return Err("HUD update repeats a cell change".into());
            }
            let cell = content
                .cards
                .iter_mut()
                .find(|card| card.id == change.card)
                .and_then(|card| card.cells.get_mut(change.index))
                .ok_or("HUD update names an unknown cell")?;
            if let Some(value) = change.color {
                cell.color = value;
            }
            if let Some(value) = change.background {
                cell.background = value;
            }
            if let Some(value) = change.label {
                cell.label = value;
            }
            if let Some(value) = change.value {
                cell.value = value;
            }
        }
        content.validate()?;
        Ok(content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Small edits retain other cards and metadata; invalid edits leave the input untouched.
    #[test]
    fn retained_edits_validate_the_complete_result() {
        let original: Hud = serde_json::from_str(
            r#"{"cards":[{"id":"fps","rows":[]},{"id":"cps","rows":[]}],"hide_scoreboard":true}"#,
        )
        .unwrap();
        let edit: HudUpdate =
            serde_json::from_str(r#"{"cards":[{"id":"fps","title":"Updated","rows":[]}] }"#)
                .unwrap();
        let updated = edit.apply(Some(&original)).unwrap();
        assert_eq!(updated.cards.len(), 2);
        assert_eq!(updated.cards[0].title, "Updated");
        assert_eq!(updated.cards[1], original.cards[1]);
        assert!(updated.hide_scoreboard);
        let invalid: HudUpdate = serde_json::from_str(r#"{"remove":["fps","fps"]}"#).unwrap();
        assert!(invalid.apply(Some(&original)).is_err());
        assert!(original.cards[0].title.is_empty());
        let remove: HudUpdate = serde_json::from_str(
            r#"{"remove":["cps"],"presentation":{"cards":[],"hide_scoreboard":false}}"#,
        )
        .unwrap();
        let updated = remove.apply(Some(&updated)).unwrap();
        assert_eq!(updated.cards.len(), 1);
        assert!(!updated.hide_scoreboard);
    }

    /// Cell edits preserve geometry and reject invalid, duplicate or missing targets atomically.
    #[test]
    fn sparse_cell_paint_retains_geometry_and_checks_the_result() {
        let original: Hud = serde_json::from_str(
            r#"{"cards":[{"id":"keys","rows":[],"cells":[{"rect":[0,0,22,22],"label":"W"}]}]}"#,
        )
        .unwrap();
        let edit: HudUpdate = serde_json::from_str(
            r#"{"cell_changes":[{"card":"keys","index":0,"color":[1,0,0,1],"label":"A"}]}"#,
        )
        .unwrap();
        let changed = edit.apply(Some(&original)).unwrap();
        assert_eq!(
            changed.cards[0].cells[0].rect,
            original.cards[0].cells[0].rect
        );
        assert_eq!(changed.cards[0].cells[0].label, "A");
        assert_eq!(changed.cards[0].cells[0].color, [1., 0., 0., 1.]);
        for changes in [
            r#"[{"card":"missing","index":0}]"#,
            r#"[{"card":"keys","index":1}]"#,
            r#"[{"card":"keys","index":0},{"card":"keys","index":0}]"#,
            r#"[{"card":"keys","index":0,"color":[2,0,0,1]}]"#,
            r#"[{"card":"keys","index":0,"label":"\n"}]"#,
        ] {
            let edit: HudUpdate =
                serde_json::from_str(&format!("{{\"cell_changes\":{changes}}}")).unwrap();
            assert!(edit.apply(Some(&original)).is_err());
        }
        assert_eq!(original.cards[0].cells[0].label, "W");
    }
}
