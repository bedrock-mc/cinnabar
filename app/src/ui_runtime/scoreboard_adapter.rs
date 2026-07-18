use protocol::{
    BossAction as ProtocolBossAction, BossColor as ProtocolBossColor, BossEvent,
    BossOverlay as ProtocolBossOverlay, ObjectiveEvent, ScoreAction as ProtocolScoreAction,
    ScoreEvent, ScoreIdentity as ProtocolScoreIdentity,
};
use ui::{
    BossAction, BossBarEvent, BossColor, BossOverlay, BossStyle, RetainedUiApply, ScoreAction,
    ScoreEntry, ScoreOwner, ScoreboardEvent,
};

use super::UiApplyOutcome;

pub(super) const fn apply_outcome(outcome: RetainedUiApply) -> UiApplyOutcome {
    match outcome {
        RetainedUiApply::Applied => UiApplyOutcome::Applied,
        RetainedUiApply::Ignored => UiApplyOutcome::IgnoredByReceiveStore,
    }
}

pub(super) fn objective(event: ObjectiveEvent) -> ScoreboardEvent {
    match event {
        ObjectiveEvent::Display {
            display_slot,
            objective_name,
            display_name,
            criteria_name,
            sort_order,
        } => ScoreboardEvent::DisplayObjective {
            display_slot,
            objective_name,
            display_name,
            criteria_name,
            sort_order,
        },
        ObjectiveEvent::Remove { objective_name } => {
            ScoreboardEvent::RemoveObjective { objective_name }
        }
    }
}

pub(super) fn score(event: ScoreEvent) -> ScoreboardEvent {
    ScoreboardEvent::Scores {
        action: match event.action {
            ProtocolScoreAction::Change => ScoreAction::Change,
            ProtocolScoreAction::Remove => ScoreAction::Remove,
        },
        entries: event
            .entries
            .iter()
            .map(|entry| ScoreEntry {
                scoreboard_id: entry.scoreboard_id,
                objective_name: entry.objective_name.clone(),
                score: entry.score,
                owner: match &entry.identity {
                    ProtocolScoreIdentity::Player(id) => ScoreOwner::Player(*id),
                    ProtocolScoreIdentity::Entity(id) => ScoreOwner::Entity(*id),
                    ProtocolScoreIdentity::FakePlayer(name) => ScoreOwner::FakePlayer(name.clone()),
                    ProtocolScoreIdentity::None => ScoreOwner::None,
                },
            })
            .collect::<Vec<_>>()
            .into(),
    }
}

pub(super) fn boss(event: BossEvent) -> BossBarEvent {
    BossBarEvent {
        target_entity_id: event.target_entity_id,
        player_id: event.player_id,
        action: match event.action {
            ProtocolBossAction::Show => BossAction::Show,
            ProtocolBossAction::RegisterPlayer => BossAction::RegisterPlayer,
            ProtocolBossAction::Hide => BossAction::Hide,
            ProtocolBossAction::UnregisterPlayer => BossAction::UnregisterPlayer,
            ProtocolBossAction::SetProgress => BossAction::SetProgress,
            ProtocolBossAction::SetTitle => BossAction::SetTitle,
            ProtocolBossAction::UpdateProperties => BossAction::UpdateProperties,
            ProtocolBossAction::Texture => BossAction::Texture,
            ProtocolBossAction::Query => BossAction::Query,
        },
        title: event.title,
        filtered_title: event.filtered_title,
        health: event.progress,
        style: BossStyle {
            color: match event.style.color {
                ProtocolBossColor::Pink => BossColor::Pink,
                ProtocolBossColor::Blue => BossColor::Blue,
                ProtocolBossColor::Red => BossColor::Red,
                ProtocolBossColor::Green => BossColor::Green,
                ProtocolBossColor::Yellow => BossColor::Yellow,
                ProtocolBossColor::Purple => BossColor::Purple,
                ProtocolBossColor::RebeccaPurple => BossColor::RebeccaPurple,
                ProtocolBossColor::White => BossColor::White,
            },
            overlay: match event.style.overlay {
                ProtocolBossOverlay::Progress => BossOverlay::Progress,
                ProtocolBossOverlay::Notched6 => BossOverlay::Notched6,
                ProtocolBossOverlay::Notched10 => BossOverlay::Notched10,
                ProtocolBossOverlay::Notched12 => BossOverlay::Notched12,
                ProtocolBossOverlay::Notched20 => BossOverlay::Notched20,
            },
            darken_sky: event.style.darken_sky,
            create_world_fog: event.style.create_world_fog,
        },
    }
}
