use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use ui::{DisplaySlot, ScoreOwner, ScoreRenderType, ScoreboardStore};

use super::UiPresentationRuntime;

pub(super) const MAX_PRESENTED_SCOREBOARD_ROWS: usize = 15;
pub(super) const MAX_PRESENTED_PLAYER_LIST_ROWS: usize = protocol::MAX_PLAYER_LIST_RECORDS;
pub(super) const MAX_PRESENTED_BELOW_NAME_ROWS: usize = ui::MAX_SCORES;
/// Provisional hearts-row placeholder cap: one ten-heart row, the Java
/// sidebar's single-row capacity, pending a version-matched native witness
/// for hearts-style criteria. Overflowing scores present only this bound.
pub(super) const MAX_PRESENTED_SCOREBOARD_HEARTS: u8 = 10;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ScoreboardPresentationScope {
    HudSidebar,
    #[allow(
        dead_code,
        reason = "the player-list projection must not render on the always-on HUD surface"
    )]
    PlayerList,
    ActorNameplate,
}

impl ScoreboardPresentationScope {
    const fn slot(self) -> DisplaySlot {
        match self {
            Self::HudSidebar => DisplaySlot::Sidebar,
            Self::PlayerList => DisplaySlot::List,
            Self::ActorNameplate => DisplaySlot::BelowName,
        }
    }

    const fn maximum_rows(self) -> usize {
        match self {
            Self::HudSidebar => MAX_PRESENTED_SCOREBOARD_ROWS,
            Self::PlayerList => MAX_PRESENTED_PLAYER_LIST_ROWS,
            Self::ActorNameplate => MAX_PRESENTED_BELOW_NAME_ROWS,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum PresentedScoreValue {
    Text(Arc<str>),
    Hearts { full_hearts: u8, half_heart: bool },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PresentedScoreboardRow {
    pub(super) label: Arc<str>,
    pub(super) value: PresentedScoreValue,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PresentedScoreboard {
    pub(super) scope: ScoreboardPresentationScope,
    pub(super) title: Arc<str>,
    pub(super) rows: Vec<PresentedScoreboardRow>,
}

#[allow(
    dead_code,
    reason = "the actor-nameplate surface consumes this after native geometry is measured"
)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PresentedBelowNameRow {
    pub(super) owner: ScoreOwner,
    pub(super) score: i32,
}

#[allow(
    dead_code,
    reason = "the actor-nameplate surface consumes this after native geometry is measured"
)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PresentedBelowNameScores {
    pub(super) scope: ScoreboardPresentationScope,
    pub(super) objective_display_name: Arc<str>,
    pub(super) rows: Vec<PresentedBelowNameRow>,
}

#[derive(Debug, Default)]
pub(super) struct PresentedScoreboardCache {
    revision: Option<u64>,
    owner_names_revision: u64,
    projection: Option<PresentedScoreboard>,
}

#[derive(Debug, Default)]
pub(super) struct ScoreboardOwnerNameAuthority {
    revision: u64,
    names: BTreeMap<i64, Arc<str>>,
}

impl UiPresentationRuntime {
    pub fn set_scoreboard_owner_names(&mut self, names: impl IntoIterator<Item = (i64, Arc<str>)>) {
        self.scoreboard_owner_names.replace(names);
    }

    pub fn refresh_scoreboard_owner_names(
        &mut self,
        store: &ScoreboardStore,
        stream: Option<&chunk_pipeline::WorldStream>,
    ) {
        self.set_scoreboard_owner_names(required_sidebar_owner_ids(store).into_iter().filter_map(
            |unique_id| {
                stream?
                    .authority()
                    .actor_display_name(unique_id)
                    .map(|name| (unique_id, name))
            },
        ));
    }
}

impl PresentedScoreboardCache {
    pub(super) fn refresh(
        &mut self,
        store: &ScoreboardStore,
        owner_names: &ScoreboardOwnerNameAuthority,
    ) -> Option<&PresentedScoreboard> {
        let revision = store.revision();
        if self.revision != Some(revision) || self.owner_names_revision != owner_names.revision {
            self.projection = project_scoreboard_for_scope(
                store,
                ScoreboardPresentationScope::HudSidebar,
                |owner| owner_names.resolve(owner),
            );
            self.revision = Some(revision);
            self.owner_names_revision = owner_names.revision;
        }
        self.projection.as_ref()
    }
}

impl ScoreboardOwnerNameAuthority {
    pub(super) fn replace(&mut self, names: impl IntoIterator<Item = (i64, Arc<str>)>) {
        let next = names
            .into_iter()
            .filter(|(_, name)| !name.is_empty())
            .collect::<BTreeMap<_, _>>();
        if self.names != next {
            self.names = next;
            self.revision = self.revision.saturating_add(1);
        }
    }

    fn resolve(&self, owner: &ScoreOwner) -> Option<Arc<str>> {
        match owner {
            ScoreOwner::Player(unique_id) | ScoreOwner::Entity(unique_id) => {
                self.names.get(unique_id).cloned()
            }
            ScoreOwner::FakePlayer(name) => Some(Arc::clone(name)),
            ScoreOwner::None => None,
        }
    }
}

/// Converts one authoritative score into its presented value under the
/// objective's render type. Hearts values are a bounded provisional
/// placeholder — the score reads as half-hearts capped to one ten-heart row —
/// because no in-repo authority fixes the native hearts-style presentation.
fn presented_score_value(render_type: ScoreRenderType, score: i32) -> PresentedScoreValue {
    match render_type {
        ScoreRenderType::Integer => PresentedScoreValue::Text(Arc::from(score.to_string())),
        ScoreRenderType::Hearts => {
            let halves = score.clamp(0, i32::from(MAX_PRESENTED_SCOREBOARD_HEARTS) * 2);
            PresentedScoreValue::Hearts {
                full_hearts: (halves / 2) as u8,
                half_heart: halves % 2 == 1,
            }
        }
    }
}

/// Bounded fallback for protocol owners no name authority can answer
/// (unloaded players, XUID-keyed scores): their raw retained numeric identity
/// stays visible instead of the row disappearing. Provisional until an exact
/// owner-name authority covers those owners.
fn fallback_owner_label(owner: &ScoreOwner) -> Arc<str> {
    match owner {
        ScoreOwner::Player(unique_id) | ScoreOwner::Entity(unique_id) => {
            Arc::from(unique_id.to_string())
        }
        ScoreOwner::FakePlayer(_) | ScoreOwner::None => Arc::from(""),
    }
}

pub(super) fn project_scoreboard_for_scope(
    store: &ScoreboardStore,
    scope: ScoreboardPresentationScope,
    mut resolve_protocol_owner: impl FnMut(&ScoreOwner) -> Option<Arc<str>>,
) -> Option<PresentedScoreboard> {
    if scope == ScoreboardPresentationScope::ActorNameplate {
        return None;
    }
    let projection = store.projection_bounded(scope.slot(), scope.maximum_rows(), |owner| {
        !matches!(owner, ScoreOwner::None)
    })?;
    let rows = projection
        .rows
        .into_iter()
        .filter_map(|row| {
            let label = match &row.owner {
                ScoreOwner::FakePlayer(label) => Arc::clone(label),
                ScoreOwner::Player(_) | ScoreOwner::Entity(_) => resolve_protocol_owner(&row.owner)
                    .unwrap_or_else(|| fallback_owner_label(&row.owner)),
                ScoreOwner::None => return None,
            };
            Some(PresentedScoreboardRow {
                label,
                value: presented_score_value(projection.render_type, row.score),
            })
        })
        .collect();
    Some(PresentedScoreboard {
        scope,
        title: projection.display_name,
        rows,
    })
}

pub(super) fn required_sidebar_owner_ids(store: &ScoreboardStore) -> Vec<i64> {
    store
        .projection_bounded(
            DisplaySlot::Sidebar,
            MAX_PRESENTED_SCOREBOARD_ROWS,
            |owner| matches!(owner, ScoreOwner::Player(_) | ScoreOwner::Entity(_)),
        )
        .map(|projection| {
            projection
                .rows
                .into_iter()
                .filter_map(|row| match row.owner {
                    ScoreOwner::Player(unique_id) | ScoreOwner::Entity(unique_id) => {
                        Some(unique_id)
                    }
                    ScoreOwner::FakePlayer(_) | ScoreOwner::None => None,
                })
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect()
        })
        .unwrap_or_default()
}

#[allow(
    dead_code,
    reason = "below-name objectives fail closed until the actor-nameplate surface has measured native geometry"
)]
pub(super) fn project_below_name_scores(
    store: &ScoreboardStore,
) -> Option<PresentedBelowNameScores> {
    let projection = store.projection_bounded(
        DisplaySlot::BelowName,
        MAX_PRESENTED_BELOW_NAME_ROWS,
        |owner| matches!(owner, ScoreOwner::Player(_) | ScoreOwner::Entity(_)),
    )?;
    Some(PresentedBelowNameScores {
        scope: ScoreboardPresentationScope::ActorNameplate,
        objective_display_name: projection.display_name,
        rows: projection
            .rows
            .into_iter()
            .map(|row| PresentedBelowNameRow {
                owner: row.owner,
                score: row.score,
            })
            .collect(),
    })
}
