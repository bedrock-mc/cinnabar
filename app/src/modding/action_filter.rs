//! Converts the component contract to the semantic router's generic verdict applier.

use super::ModHosts;
use client_state::SemanticInputSnapshot;
use semantic_input::action_filter as input;

/// Completes owned releases on native-only and suspended paths without inventing presses.
pub(super) fn inactive(
    state: &mut input::ActionFilterState,
    input: Option<&mut SemanticInputSnapshot>,
    time_us: u128,
) {
    state.reset();
    if let Some(input) = input {
        input.filter_actions(state, None, clock(time_us), false, |_| None);
    }
}

/// Resolves one owner and applies its return value before gameplay or legacy policies.
pub(super) fn apply(
    state: &mut input::ActionFilterState,
    input: Option<&mut SemanticInputSnapshot>,
    hosts: &mut ModHosts,
    enabled: bool,
    time_us: u128,
    repeat: bool,
) -> (bool, Option<(usize, String)>) {
    let Some(input) = input else {
        state.reset();
        return (false, None);
    };
    let owner = enabled.then(|| hosts.action_filter_owner()).flatten();
    let identity = owner.and_then(|i| hosts.host(i).action_filter_identity());
    let mut error = None;
    let pulse = input.filter_actions(
        state,
        identity,
        clock(time_us),
        enabled && repeat,
        |frame| {
            let index = owner?;
            match hosts.host_mut(index).filter_actions(to_guest(frame)) {
                Ok(verdict) => Some(input::ActionVerdict {
                    accepted: verdict.accepted,
                    release: [verdict.release.attack, verdict.release.item_use],
                }),
                Err(failure) => {
                    error = Some((index, format!("{failure:#}")));
                    None
                }
            }
        },
    );
    (pulse, error)
}

/// Keeps the contract clock monotonic even beyond the u64 microsecond range.
fn clock(time_us: u128) -> u64 {
    u64::try_from(time_us).unwrap_or(u64::MAX)
}

/// Maps only the versioned actions and source kinds admitted by the component contract.
fn to_guest(frame: &input::ActionFilterFrame) -> mod_host::ActionFrame {
    let held = |value: [bool; 2]| mod_host::ActionHeld {
        attack: value[0],
        item_use: value[1],
    };
    mod_host::ActionFrame {
        epoch: frame.epoch,
        time_us: frame.time_us,
        physical: held(frame.physical),
        logical: held(frame.logical),
        transitions: frame
            .transitions
            .iter()
            .map(|t| mod_host::ActionTransition {
                action: match t.action {
                    input::FilterAction::Attack => mod_host::FilteredAction::Attack,
                    input::FilterAction::ItemUse => mod_host::FilteredAction::ItemUse,
                },
                kind: match t.kind {
                    input::TransitionKind::Press => mod_host::TransitionKind::Press,
                    input::TransitionKind::Release => mod_host::TransitionKind::Release,
                    input::TransitionKind::Repeat => mod_host::TransitionKind::Repeat,
                },
            })
            .collect(),
    }
}
