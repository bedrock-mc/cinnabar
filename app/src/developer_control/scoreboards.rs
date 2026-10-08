//! Bounded scoreboard text inspection, evaluated only on a state request.

use serde_json::{Value, json};
use ui::{DisplaySlot, ScoreOwner, ScoreboardStore};

const MAX_LISTED_ROWS: usize = 32;

pub(super) fn snapshot(store: &ScoreboardStore) -> Option<Value> {
    let projection = store.projection_bounded(DisplaySlot::Sidebar, MAX_LISTED_ROWS, |_| true)?;
    let rows: Vec<_> = projection
        .rows
        .iter()
        .map(|row| {
            let (owner, text, unique_id) = match &row.owner {
                ScoreOwner::FakePlayer(text) => ("text", Some(text.as_ref()), None),
                ScoreOwner::Player(id) => ("player", None, Some(*id)),
                ScoreOwner::Entity(id) => ("entity", None, Some(*id)),
                ScoreOwner::None => ("none", None, None),
            };
            json!({
                "entry_id": row.identity.entry_id,
                "score": row.score,
                "owner": owner,
                "text": text,
                "unique_id": unique_id,
            })
        })
        .collect();
    Some(json!({
        "revision": store.revision(),
        "objective": projection.objective_name.as_ref(),
        "title": projection.display_name.as_ref(),
        "rows": rows,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ui::{ScoreAction, ScoreEntry, ScoreboardEvent};

    #[test]
    fn sidebar_inspection_preserves_formatted_private_use_text_and_bounds_rows() {
        let mut store = ScoreboardStore::default();
        store
            .apply(
                1,
                ScoreboardEvent::DisplayObjective {
                    display_slot: "sidebar".into(),
                    objective_name: "diagnostic".into(),
                    display_name: "Title".into(),
                    criteria_name: "dummy".into(),
                    sort_order: 1,
                },
            )
            .unwrap();
        let text = "§bVIP Level: §r\u{e123} 1";
        let entries: Vec<_> = (0..MAX_LISTED_ROWS + 1)
            .map(|index| ScoreEntry {
                action: ScoreAction::Change,
                scoreboard_id: index as i64,
                objective_name: "diagnostic".into(),
                score: index as i32,
                owner: ScoreOwner::FakePlayer(text.into()),
            })
            .collect();
        store
            .apply(
                2,
                ScoreboardEvent::Scores {
                    entries: entries.into(),
                },
            )
            .unwrap();
        let state = snapshot(&store).unwrap();
        let rows = state["rows"].as_array().unwrap();
        assert_eq!(rows.len(), MAX_LISTED_ROWS);
        assert!(rows.iter().all(|row| row["text"] == text));
        assert_eq!(rows[0]["score"], MAX_LISTED_ROWS);
        assert_eq!(rows[MAX_LISTED_ROWS - 1]["score"], 1);
        assert!(snapshot(&ScoreboardStore::default()).is_none());
    }
}
