use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

pub const MAX_OBJECTIVES: usize = 128;
pub const MAX_SCORES: usize = 8_192;
pub const MAX_BOSS_BARS: usize = 64;
pub const MAX_BOSS_PLAYER_MEMBERSHIPS: usize = 8_192;
pub const MAX_RETAINED_UI_TEXT_FIELD_BYTES: usize = crate::UiLimits::MAX_TEXT_BYTES;
pub const MAX_SCOREBOARD_RETAINED_TEXT_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_BOSS_RETAINED_TEXT_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RetainedUiApply {
    Applied,
    Ignored,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetainedUiSequenceError {
    pub previous: u64,
    pub actual: u64,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum DisplaySlot {
    Sidebar,
    List,
    BelowName,
}

impl DisplaySlot {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "sidebar" => Some(Self::Sidebar),
            "list" => Some(Self::List),
            "belowname" => Some(Self::BelowName),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScoreSortOrder {
    Ascending,
    Descending,
    Unsupported(i32),
}

impl From<i32> for ScoreSortOrder {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Ascending,
            1 => Self::Descending,
            value => Self::Unsupported(value),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScoreAction {
    Change,
    Remove,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScoreOwner {
    Player(i64),
    Entity(i64),
    FakePlayer(Arc<str>),
    None,
}

impl ScoreOwner {
    fn retained_text_bytes(&self) -> usize {
        match self {
            Self::FakePlayer(name) => name.len(),
            Self::Player(_) | Self::Entity(_) | Self::None => 0,
        }
    }

    fn text_is_bounded(&self) -> bool {
        match self {
            Self::FakePlayer(name) => text_is_bounded(name),
            Self::Player(_) | Self::Entity(_) | Self::None => true,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreEntry {
    pub scoreboard_id: i64,
    pub objective_name: Arc<str>,
    pub score: i32,
    pub owner: ScoreOwner,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScoreboardEvent {
    DisplayObjective {
        display_slot: Arc<str>,
        objective_name: Arc<str>,
        display_name: Arc<str>,
        criteria_name: Arc<str>,
        sort_order: i32,
    },
    RemoveObjective {
        objective_name: Arc<str>,
    },
    Scores {
        action: ScoreAction,
        entries: Arc<[ScoreEntry]>,
    },
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ScoreIdentity {
    pub objective_name: Arc<str>,
    pub entry_id: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreRow {
    pub identity: ScoreIdentity,
    pub score: i32,
    pub owner: ScoreOwner,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreboardProjection {
    pub slot: DisplaySlot,
    pub objective_name: Arc<str>,
    pub display_name: Arc<str>,
    pub criteria_name: Arc<str>,
    pub sort_order: ScoreSortOrder,
    pub rows: Vec<ScoreRow>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ScoreboardDiagnostics {
    pub stale_sequences: u64,
    pub unsupported_display_slots: u64,
    pub unsupported_sort_orders: u64,
    pub missing_objectives: u64,
    pub missing_scores: u64,
    pub objective_limit_rejections: u64,
    pub score_event_limit_rejections: u64,
    pub score_limit_rejections: u64,
    pub text_field_rejections: u64,
    pub text_budget_rejections: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ObjectiveState {
    display_name: Arc<str>,
    criteria_name: Arc<str>,
    sort_order: ScoreSortOrder,
    scores: BTreeMap<i64, StoredScore>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct StoredScore {
    score: i32,
    owner: ScoreOwner,
}

#[derive(Clone, Debug, Default)]
pub struct ScoreboardStore {
    revision: u64,
    last_sequence: Option<u64>,
    objectives: BTreeMap<Arc<str>, ObjectiveState>,
    slots: BTreeMap<DisplaySlot, Arc<str>>,
    score_count: usize,
    retained_text_bytes: usize,
    diagnostics: ScoreboardDiagnostics,
}

impl ScoreboardStore {
    pub fn clear(&mut self) {
        let revision = self.revision.saturating_add(1);
        *self = Self::default();
        self.revision = revision;
    }

    pub const fn revision(&self) -> u64 {
        self.revision
    }

    pub fn objective_count(&self) -> usize {
        self.objectives.len()
    }

    pub const fn score_count(&self) -> usize {
        self.score_count
    }

    pub const fn retained_text_bytes(&self) -> usize {
        self.retained_text_bytes
    }

    pub const fn diagnostics(&self) -> &ScoreboardDiagnostics {
        &self.diagnostics
    }

    pub fn apply(
        &mut self,
        sequence: u64,
        event: ScoreboardEvent,
    ) -> Result<RetainedUiApply, RetainedUiSequenceError> {
        if let Some(previous) = self.last_sequence
            && sequence <= previous
        {
            self.diagnostics.stale_sequences = self.diagnostics.stale_sequences.saturating_add(1);
            return Err(RetainedUiSequenceError {
                previous,
                actual: sequence,
            });
        }
        let result = match event {
            ScoreboardEvent::DisplayObjective {
                display_slot,
                objective_name,
                display_name,
                criteria_name,
                sort_order,
            } => self.display_objective(
                display_slot,
                objective_name,
                display_name,
                criteria_name,
                sort_order,
            ),
            ScoreboardEvent::RemoveObjective { objective_name } => {
                self.remove_objective(&objective_name)
            }
            ScoreboardEvent::Scores { action, entries } => self.apply_scores(action, &entries),
        };
        self.last_sequence = Some(sequence);
        if result == RetainedUiApply::Applied {
            self.revision = self.revision.saturating_add(1);
        }
        Ok(result)
    }

    pub fn sidebar(&self) -> Option<ScoreboardProjection> {
        self.projection(DisplaySlot::Sidebar)
    }

    pub fn list(&self) -> Option<ScoreboardProjection> {
        self.projection(DisplaySlot::List)
    }

    pub fn below_name(&self) -> Option<ScoreboardProjection> {
        self.projection(DisplaySlot::BelowName)
    }

    /// The single below-name score for one entity, the way a nameplate
    /// presents it: the owner's value under the below-name objective, with the
    /// objective display name as the suffix label. `None` when no below-name
    /// objective is displayed or the owner has no score in it.
    pub fn below_name_for_owner(&self, owner: &ScoreOwner) -> Option<(i32, Arc<str>)> {
        let objective_name = self.slots.get(&DisplaySlot::BelowName)?;
        let objective = self.objectives.get(objective_name)?;
        let score = objective
            .scores
            .values()
            .find(|score| &score.owner == owner)?;
        Some((score.score, Arc::clone(&objective.display_name)))
    }

    /// The list-slot score for one player entry, as the player list presents
    /// it beside each name.
    pub fn list_score_for_owner(&self, owner: &ScoreOwner) -> Option<i32> {
        let objective_name = self.slots.get(&DisplaySlot::List)?;
        let objective = self.objectives.get(objective_name)?;
        objective
            .scores
            .values()
            .find(|score| &score.owner == owner)
            .map(|score| score.score)
    }

    pub fn projection(&self, slot: DisplaySlot) -> Option<ScoreboardProjection> {
        self.projection_bounded(slot, MAX_SCORES, |_| true)
    }

    pub fn projection_bounded(
        &self,
        slot: DisplaySlot,
        maximum_rows: usize,
        mut include: impl FnMut(&ScoreOwner) -> bool,
    ) -> Option<ScoreboardProjection> {
        let objective_name = self.slots.get(&slot)?;
        let objective = self.objectives.get(objective_name)?;
        if matches!(objective.sort_order, ScoreSortOrder::Unsupported(_)) {
            return None;
        }
        let maximum_rows = maximum_rows.min(MAX_SCORES);
        let mut selected =
            Vec::<(&i64, &StoredScore)>::with_capacity(maximum_rows.min(objective.scores.len()));
        for (entry_id, score) in objective
            .scores
            .iter()
            .filter(|(_, score)| include(&score.owner))
        {
            let position = selected
                .binary_search_by(|candidate| {
                    let (selected_id, selected_score) = *candidate;
                    compare_scores(
                        objective.sort_order,
                        *selected_id,
                        selected_score,
                        *entry_id,
                        score,
                    )
                })
                .unwrap_or_else(|position| position);
            if position >= maximum_rows {
                continue;
            }
            if selected.len() == maximum_rows {
                selected.pop();
            }
            selected.insert(position, (entry_id, score));
        }
        let rows = selected
            .into_iter()
            .map(|(entry_id, score)| ScoreRow {
                identity: ScoreIdentity {
                    objective_name: Arc::clone(objective_name),
                    entry_id: *entry_id,
                },
                score: score.score,
                owner: score.owner.clone(),
            })
            .collect();
        Some(ScoreboardProjection {
            slot,
            objective_name: Arc::clone(objective_name),
            display_name: Arc::clone(&objective.display_name),
            criteria_name: Arc::clone(&objective.criteria_name),
            sort_order: objective.sort_order,
            rows,
        })
    }

    fn display_objective(
        &mut self,
        display_slot: Arc<str>,
        objective_name: Arc<str>,
        display_name: Arc<str>,
        criteria_name: Arc<str>,
        raw_sort_order: i32,
    ) -> RetainedUiApply {
        if ![
            &display_slot,
            &objective_name,
            &display_name,
            &criteria_name,
        ]
        .into_iter()
        .all(|text| text_is_bounded(text))
        {
            self.diagnostics.text_field_rejections =
                self.diagnostics.text_field_rejections.saturating_add(1);
            return RetainedUiApply::Ignored;
        }
        let Some(slot) = DisplaySlot::parse(&display_slot) else {
            self.diagnostics.unsupported_display_slots =
                self.diagnostics.unsupported_display_slots.saturating_add(1);
            return RetainedUiApply::Ignored;
        };
        let sort_order = ScoreSortOrder::from(raw_sort_order);
        let old_bytes = self.objectives.get(&objective_name).map_or(0, |objective| {
            objective.display_name.len() + objective.criteria_name.len()
        });
        let name_bytes = if self.objectives.contains_key(&objective_name) {
            0
        } else {
            objective_name.len()
        };
        if !self.objectives.contains_key(&objective_name) && self.objectives.len() >= MAX_OBJECTIVES
        {
            self.diagnostics.objective_limit_rejections = self
                .diagnostics
                .objective_limit_rejections
                .saturating_add(1);
            return RetainedUiApply::Ignored;
        }
        let new_text_bytes = self
            .retained_text_bytes
            .saturating_sub(old_bytes)
            .saturating_add(name_bytes)
            .saturating_add(display_name.len())
            .saturating_add(criteria_name.len());
        if new_text_bytes > MAX_SCOREBOARD_RETAINED_TEXT_BYTES {
            self.diagnostics.text_budget_rejections =
                self.diagnostics.text_budget_rejections.saturating_add(1);
            return RetainedUiApply::Ignored;
        }
        match self.objectives.get_mut(&objective_name) {
            Some(objective) => {
                objective.display_name = display_name;
                objective.criteria_name = criteria_name;
                objective.sort_order = sort_order;
            }
            None => {
                self.objectives.insert(
                    Arc::clone(&objective_name),
                    ObjectiveState {
                        display_name,
                        criteria_name,
                        sort_order,
                        scores: BTreeMap::new(),
                    },
                );
            }
        }
        self.slots.insert(slot, objective_name);
        self.retained_text_bytes = new_text_bytes;
        if matches!(sort_order, ScoreSortOrder::Unsupported(_)) {
            self.diagnostics.unsupported_sort_orders =
                self.diagnostics.unsupported_sort_orders.saturating_add(1);
        }
        RetainedUiApply::Applied
    }

    fn remove_objective(&mut self, objective_name: &str) -> RetainedUiApply {
        if !text_is_bounded(objective_name) {
            self.diagnostics.text_field_rejections =
                self.diagnostics.text_field_rejections.saturating_add(1);
            return RetainedUiApply::Ignored;
        }
        let Some(objective) = self.objectives.remove(objective_name) else {
            self.diagnostics.missing_objectives =
                self.diagnostics.missing_objectives.saturating_add(1);
            return RetainedUiApply::Ignored;
        };
        self.slots
            .retain(|_, displayed| displayed.as_ref() != objective_name);
        self.score_count = self.score_count.saturating_sub(objective.scores.len());
        let score_bytes = objective
            .scores
            .values()
            .map(|score| score.owner.retained_text_bytes())
            .sum::<usize>();
        self.retained_text_bytes = self.retained_text_bytes.saturating_sub(
            objective_name.len()
                + objective.display_name.len()
                + objective.criteria_name.len()
                + score_bytes,
        );
        RetainedUiApply::Applied
    }

    fn apply_scores(&mut self, action: ScoreAction, entries: &[ScoreEntry]) -> RetainedUiApply {
        if entries.len() > MAX_SCORES {
            self.diagnostics.score_event_limit_rejections = self
                .diagnostics
                .score_event_limit_rejections
                .saturating_add(1);
            return RetainedUiApply::Ignored;
        }
        let mut staged = BTreeMap::<(Arc<str>, i64), Option<StoredScore>>::new();
        for entry in entries {
            if !text_is_bounded(&entry.objective_name) || !entry.owner.text_is_bounded() {
                self.diagnostics.text_field_rejections =
                    self.diagnostics.text_field_rejections.saturating_add(1);
                return RetainedUiApply::Ignored;
            }
            let Some(objective) = self.objectives.get(&entry.objective_name) else {
                self.diagnostics.missing_objectives =
                    self.diagnostics.missing_objectives.saturating_add(1);
                return RetainedUiApply::Ignored;
            };
            let key = (Arc::clone(&entry.objective_name), entry.scoreboard_id);
            match action {
                ScoreAction::Change => {
                    staged.insert(
                        key,
                        Some(StoredScore {
                            score: entry.score,
                            owner: entry.owner.clone(),
                        }),
                    );
                }
                ScoreAction::Remove => {
                    let exists = staged.get(&key).map_or_else(
                        || objective.scores.contains_key(&entry.scoreboard_id),
                        Option::is_some,
                    );
                    if exists {
                        staged.insert(key, None);
                    } else {
                        self.diagnostics.missing_scores =
                            self.diagnostics.missing_scores.saturating_add(1);
                        return RetainedUiApply::Ignored;
                    }
                }
            }
        }
        if staged.is_empty() {
            return RetainedUiApply::Ignored;
        }

        let mut next_score_count = self.score_count;
        let mut next_text_bytes = self.retained_text_bytes;
        for ((objective_name, entry_id), replacement) in &staged {
            let existing = self
                .objectives
                .get(objective_name)
                .and_then(|objective| objective.scores.get(entry_id));
            next_text_bytes = next_text_bytes
                .saturating_sub(existing.map_or(0, |score| score.owner.retained_text_bytes()))
                .saturating_add(
                    replacement
                        .as_ref()
                        .map_or(0, |score| score.owner.retained_text_bytes()),
                );
            match (existing.is_some(), replacement.is_some()) {
                (false, true) => next_score_count = next_score_count.saturating_add(1),
                (true, false) => next_score_count = next_score_count.saturating_sub(1),
                _ => {}
            }
        }
        if next_score_count > MAX_SCORES {
            self.diagnostics.score_limit_rejections =
                self.diagnostics.score_limit_rejections.saturating_add(1);
            return RetainedUiApply::Ignored;
        }
        if next_text_bytes > MAX_SCOREBOARD_RETAINED_TEXT_BYTES {
            self.diagnostics.text_budget_rejections =
                self.diagnostics.text_budget_rejections.saturating_add(1);
            return RetainedUiApply::Ignored;
        }
        if staged
            .keys()
            .any(|(objective_name, _)| !self.objectives.contains_key(objective_name))
        {
            self.diagnostics.missing_objectives =
                self.diagnostics.missing_objectives.saturating_add(1);
            return RetainedUiApply::Ignored;
        }
        for ((objective_name, entry_id), replacement) in staged {
            let Some(objective) = self.objectives.get_mut(&objective_name) else {
                return RetainedUiApply::Ignored;
            };
            match replacement {
                Some(score) => {
                    objective.scores.insert(entry_id, score);
                }
                None => {
                    objective.scores.remove(&entry_id);
                }
            }
        }
        self.score_count = next_score_count;
        self.retained_text_bytes = next_text_bytes;
        RetainedUiApply::Applied
    }
}

fn compare_scores(
    sort_order: ScoreSortOrder,
    left_id: i64,
    left: &StoredScore,
    right_id: i64,
    right: &StoredScore,
) -> Ordering {
    let score_order = match sort_order {
        ScoreSortOrder::Ascending => left.score.cmp(&right.score),
        ScoreSortOrder::Descending => right.score.cmp(&left.score),
        ScoreSortOrder::Unsupported(_) => Ordering::Equal,
    };
    score_order.then_with(|| left_id.cmp(&right_id))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BossAction {
    Show,
    RegisterPlayer,
    Hide,
    UnregisterPlayer,
    SetProgress,
    SetTitle,
    UpdateProperties,
    Texture,
    Query,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BossColor {
    Pink,
    Blue,
    Red,
    Green,
    Yellow,
    Purple,
    RebeccaPurple,
    White,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BossOverlay {
    Progress,
    Notched6,
    Notched10,
    Notched12,
    Notched20,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BossStyle {
    pub color: BossColor,
    pub overlay: BossOverlay,
    pub darken_sky: Option<bool>,
    pub create_world_fog: Option<bool>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BossBarEvent {
    pub target_entity_id: i64,
    pub player_id: i64,
    pub action: BossAction,
    pub title: Arc<str>,
    pub filtered_title: Arc<str>,
    pub health: f32,
    pub style: BossStyle,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BossBarView {
    pub target_entity_id: i64,
    pub title: Arc<str>,
    pub filtered_title: Arc<str>,
    pub health: f32,
    pub style: BossStyle,
    pub registered_players: Arc<[i64]>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BossBarDiagnostics {
    pub stale_sequences: u64,
    pub missing_bars: u64,
    pub missing_memberships: u64,
    pub bar_limit_rejections: u64,
    pub membership_limit_rejections: u64,
    pub text_field_rejections: u64,
    pub text_budget_rejections: u64,
    pub invalid_health_rejections: u64,
    pub ignored_queries: u64,
}

#[derive(Clone, Debug, PartialEq)]
struct BossBarState {
    first_show_sequence: u64,
    title: Arc<str>,
    filtered_title: Arc<str>,
    health: f32,
    style: BossStyle,
    registered_players: BTreeSet<i64>,
}

#[derive(Clone, Debug, Default)]
pub struct BossBarStore {
    last_sequence: Option<u64>,
    bars: BTreeMap<i64, BossBarState>,
    membership_count: usize,
    retained_text_bytes: usize,
    diagnostics: BossBarDiagnostics,
}

impl BossBarStore {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub const fn retained_text_bytes(&self) -> usize {
        self.retained_text_bytes
    }

    pub const fn diagnostics(&self) -> &BossBarDiagnostics {
        &self.diagnostics
    }

    pub fn stacked(&self) -> Vec<BossBarView> {
        let mut bars = self.bars.iter().collect::<Vec<_>>();
        bars.sort_by_key(|(entity_id, bar)| (bar.first_show_sequence, **entity_id));
        bars.into_iter()
            .map(|(target_entity_id, bar)| BossBarView {
                target_entity_id: *target_entity_id,
                title: Arc::clone(&bar.title),
                filtered_title: Arc::clone(&bar.filtered_title),
                health: bar.health,
                style: bar.style,
                registered_players: bar
                    .registered_players
                    .iter()
                    .copied()
                    .collect::<Vec<_>>()
                    .into(),
            })
            .collect()
    }

    pub fn apply(
        &mut self,
        sequence: u64,
        event: BossBarEvent,
    ) -> Result<RetainedUiApply, RetainedUiSequenceError> {
        if let Some(previous) = self.last_sequence
            && sequence <= previous
        {
            self.diagnostics.stale_sequences = self.diagnostics.stale_sequences.saturating_add(1);
            return Err(RetainedUiSequenceError {
                previous,
                actual: sequence,
            });
        }
        let result = self.apply_event(sequence, event);
        self.last_sequence = Some(sequence);
        Ok(result)
    }

    fn apply_event(&mut self, sequence: u64, event: BossBarEvent) -> RetainedUiApply {
        if !event.health.is_finite() {
            self.diagnostics.invalid_health_rejections =
                self.diagnostics.invalid_health_rejections.saturating_add(1);
            return RetainedUiApply::Ignored;
        }
        match event.action {
            BossAction::Show => self.show(sequence, event),
            BossAction::Hide => self.hide(event.target_entity_id),
            BossAction::RegisterPlayer => {
                let Some(bar) = self.bars.get_mut(&event.target_entity_id) else {
                    return self.note_missing_bar();
                };
                if bar.registered_players.contains(&event.player_id) {
                    return RetainedUiApply::Applied;
                }
                if self.membership_count >= MAX_BOSS_PLAYER_MEMBERSHIPS {
                    self.diagnostics.membership_limit_rejections = self
                        .diagnostics
                        .membership_limit_rejections
                        .saturating_add(1);
                    return RetainedUiApply::Ignored;
                }
                bar.registered_players.insert(event.player_id);
                self.membership_count += 1;
                RetainedUiApply::Applied
            }
            BossAction::UnregisterPlayer => {
                let Some(bar) = self.bars.get_mut(&event.target_entity_id) else {
                    return self.note_missing_bar();
                };
                if bar.registered_players.remove(&event.player_id) {
                    self.membership_count = self.membership_count.saturating_sub(1);
                    RetainedUiApply::Applied
                } else {
                    self.diagnostics.missing_memberships =
                        self.diagnostics.missing_memberships.saturating_add(1);
                    RetainedUiApply::Ignored
                }
            }
            BossAction::SetProgress => {
                let Some(bar) = self.bars.get_mut(&event.target_entity_id) else {
                    return self.note_missing_bar();
                };
                bar.health = event.health;
                RetainedUiApply::Applied
            }
            BossAction::SetTitle => self.set_title(event),
            BossAction::UpdateProperties | BossAction::Texture => {
                let Some(bar) = self.bars.get_mut(&event.target_entity_id) else {
                    return self.note_missing_bar();
                };
                bar.style = event.style;
                RetainedUiApply::Applied
            }
            BossAction::Query => {
                self.diagnostics.ignored_queries =
                    self.diagnostics.ignored_queries.saturating_add(1);
                RetainedUiApply::Ignored
            }
        }
    }

    fn show(&mut self, sequence: u64, event: BossBarEvent) -> RetainedUiApply {
        if !text_is_bounded(&event.title) || !text_is_bounded(&event.filtered_title) {
            self.diagnostics.text_field_rejections =
                self.diagnostics.text_field_rejections.saturating_add(1);
            return RetainedUiApply::Ignored;
        }
        if !self.bars.contains_key(&event.target_entity_id) && self.bars.len() >= MAX_BOSS_BARS {
            self.diagnostics.bar_limit_rejections =
                self.diagnostics.bar_limit_rejections.saturating_add(1);
            return RetainedUiApply::Ignored;
        }
        let old_bytes = self
            .bars
            .get(&event.target_entity_id)
            .map_or(0, |bar| bar.title.len() + bar.filtered_title.len());
        let next_bytes = self
            .retained_text_bytes
            .saturating_sub(old_bytes)
            .saturating_add(event.title.len())
            .saturating_add(event.filtered_title.len());
        if next_bytes > MAX_BOSS_RETAINED_TEXT_BYTES {
            self.diagnostics.text_budget_rejections =
                self.diagnostics.text_budget_rejections.saturating_add(1);
            return RetainedUiApply::Ignored;
        }
        match self.bars.get_mut(&event.target_entity_id) {
            Some(bar) => {
                bar.title = event.title;
                bar.filtered_title = event.filtered_title;
                bar.health = event.health;
                bar.style = event.style;
            }
            None => {
                self.bars.insert(
                    event.target_entity_id,
                    BossBarState {
                        first_show_sequence: sequence,
                        title: event.title,
                        filtered_title: event.filtered_title,
                        health: event.health,
                        style: event.style,
                        registered_players: BTreeSet::new(),
                    },
                );
            }
        }
        self.retained_text_bytes = next_bytes;
        RetainedUiApply::Applied
    }

    fn hide(&mut self, target_entity_id: i64) -> RetainedUiApply {
        let Some(bar) = self.bars.remove(&target_entity_id) else {
            return self.note_missing_bar();
        };
        self.membership_count = self
            .membership_count
            .saturating_sub(bar.registered_players.len());
        self.retained_text_bytes = self
            .retained_text_bytes
            .saturating_sub(bar.title.len() + bar.filtered_title.len());
        RetainedUiApply::Applied
    }

    fn set_title(&mut self, event: BossBarEvent) -> RetainedUiApply {
        if !text_is_bounded(&event.title) || !text_is_bounded(&event.filtered_title) {
            self.diagnostics.text_field_rejections =
                self.diagnostics.text_field_rejections.saturating_add(1);
            return RetainedUiApply::Ignored;
        }
        let Some(bar) = self.bars.get_mut(&event.target_entity_id) else {
            return self.note_missing_bar();
        };
        let next_bytes = self
            .retained_text_bytes
            .saturating_sub(bar.title.len() + bar.filtered_title.len())
            .saturating_add(event.title.len() + event.filtered_title.len());
        if next_bytes > MAX_BOSS_RETAINED_TEXT_BYTES {
            self.diagnostics.text_budget_rejections =
                self.diagnostics.text_budget_rejections.saturating_add(1);
            return RetainedUiApply::Ignored;
        }
        bar.title = event.title;
        bar.filtered_title = event.filtered_title;
        self.retained_text_bytes = next_bytes;
        RetainedUiApply::Applied
    }

    fn note_missing_bar(&mut self) -> RetainedUiApply {
        self.diagnostics.missing_bars = self.diagnostics.missing_bars.saturating_add(1);
        RetainedUiApply::Ignored
    }
}

fn text_is_bounded(text: &str) -> bool {
    text.len() <= MAX_RETAINED_UI_TEXT_FIELD_BYTES
}
