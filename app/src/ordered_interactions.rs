//! Admits mutating input in source order through the existing gameplay owners.

use bevy::prelude::*;
use client_interaction::{
    block_use::BlockUseRuntime, item_use::ItemUseRuntime, melee::MeleeRuntime,
    movement::MovementTicker,
};
use client_presentation::local_player::{InteractionOriginSnapshot, LocalViewPose};
use client_state::SemanticInputSnapshot;
use gameplay::ordered_interactions::{InteractionScope, Invalidation, OrderedInteractions};
use semantic_input::{
    Action, ActionButtons, ActionEvent, ActionSnapshot, MAX_PENDING_ACTION_EVENTS,
};
use std::sync::Arc;

/// The owning action either keeps its attempt, completes, or retains an accepted carrier fence.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Progress {
    Pending,
    Complete,
    Carrier,
}

/// Transfers an attempt back to its sole queue owner after one admission step.
pub(crate) struct Step<T> {
    pub attempt: T,
    pub progress: Progress,
}

impl<T> Step<T> {
    /// Returns attempt ownership together with its admission outcome.
    pub(crate) fn new(attempt: T, progress: Progress) -> Self {
        Self { attempt, progress }
    }

    /// Changes only the attempt carrier while retaining the admission decision.
    fn map<U>(self, map: impl FnOnce(T) -> U) -> Step<U> {
        Step::new(map(self.attempt), self.progress)
    }
}

/// The resolved edge uses one captured context without mirroring its facts.
#[derive(Clone)]
pub(crate) struct InteractionInput {
    pub edge: ActionEvent,
    pub context: CapturedContext,
}

/// Input authority, pose and assistance captured together before action dispatch.
#[derive(Clone)]
pub(crate) struct CapturedContext {
    pub mode: semantic_input::InputMode,
    pub identity: (
        semantic_input::identity::InputAuthorityGeneration,
        semantic_input::identity::InputSequence,
    ),
    pub origin: InteractionOriginSnapshot,
    pub view: LocalViewPose,
    pub aim: Option<Quat>,
}

/// Geometry and context are retained once per batch, without its event vector.
struct CapturedFrame {
    context: CapturedContext,
    attack: crate::melee::ordered::CapturedAttack,
    selection_authority: inventory::SelectionAuthority,
    policy: AdmissionPolicy,
}

/// Every unadmitted action shares the same current mode and capability check.
#[derive(PartialEq)]
struct AdmissionPolicy {
    mode: Option<protocol::PlayerGameMode>,
    capabilities: Option<client_world::game_mode_capabilities::GameModeCapabilities>,
}

impl AdmissionPolicy {
    /// Rebuilds admission policy from the authoritative player facts.
    fn observe(player: &client_state::PlayerRuntime) -> Self {
        Self {
            mode: player.facts.player_game_mode(),
            capabilities: player.facts.game_mode_capabilities(),
        }
    }
}

#[derive(Default)]
struct UseAttempt {
    block: crate::block_use::ordered::BlockAttempt,
    block_done: bool,
    air: crate::item_use::ordered::AirAttempt,
}

enum Work {
    Attack(Box<crate::melee::ordered::AttackAttempt>),
    Use(Box<UseAttempt>),
    Selection(Box<crate::hotbar::ordered::SelectionAttempt>),
    Pick(Option<crate::pick_block::PickAttempt>),
    Release,
}

struct Event {
    edge: ActionEvent,
    source: Arc<CapturedFrame>,
    work: Work,
}

/// The domain queue owns ordering; ECS state only adapts captured render facts to action owners.
#[derive(Resource, Default)]
pub(crate) struct InteractionRuntime {
    queue: OrderedInteractions<Event>,
    buttons: ActionButtons,
    ingested: usize,
    remaining: usize,
    overflowed_batches: u64,
}

/// Reports whether pending input still owns the mutating-action cursor.
pub(crate) fn is_blocked(runtime: Option<&InteractionRuntime>) -> bool {
    runtime.is_some_and(|runtime| !runtime.queue.is_empty() || runtime.queue.quarantined())
}

/// Reports the cursor state without requiring legacy test fixtures to install this resource.
pub(crate) fn pending(world: &World) -> bool {
    is_blocked(world.get_resource::<InteractionRuntime>())
}

/// Keeps held maintenance at the last admitted edge while a later action is blocked.
pub(crate) fn held(runtime: Option<&InteractionRuntime>, action: Action, final_held: bool) -> bool {
    runtime
        .filter(|runtime| !runtime.queue.is_empty() || runtime.queue.quarantined())
        .map_or(final_held, |runtime| runtime.buttons.held(action))
}

/// Cancels pending gestures at an authority boundary without deleting accepted mining carriers.
fn cancel_pending_owners(world: &mut World) {
    world.resource_mut::<MeleeRuntime>().cancel();
    world.resource_mut::<BlockUseRuntime>().clear_press();
    world
        .resource_mut::<ItemUseRuntime>()
        .cancel_pending_input();
}

/// Captures a batch after UI authority, keeping the existing published picking phase.
fn capture(world: &mut World, input: &ActionSnapshot) -> Arc<CapturedFrame> {
    let position = world
        .resource::<MovementTicker>()
        .interaction_authority_identity()
        .1;
    if !world
        .resource::<crate::melee::MeleeTarget>()
        .has_published_inputs(position)
    {
        world
            .run_system_cached(crate::melee::prepare_melee)
            .expect("ordered pick preparation");
    }
    let attack = world
        .run_system_cached(crate::melee::ordered::capture)
        .expect("published attack pick");
    let aim = world
        .get_resource::<client_presentation::aim_assist::AimAssistFrame>()
        .zip(world.get_resource::<client_presentation::camera::ServerCameraView>())
        .and_then(|(aim, camera)| {
            client_interaction::camera::aim_assist::action_rotation(aim, camera)
        });
    Arc::new(CapturedFrame {
        context: CapturedContext {
            mode: input.input_mode,
            identity: (input.authority_generation, input.frame_sequence),
            origin: world.resource::<InteractionOriginSnapshot>().clone(),
            view: *world.resource::<LocalViewPose>(),
            aim,
        },
        attack,
        selection_authority: world
            .resource::<client_state::PlayerRuntime>()
            .inventory
            .selection_authority(),
        policy: AdmissionPolicy::observe(world.resource::<client_state::PlayerRuntime>()),
    })
}

/// Classifies only mutating actions; bindings and their order have already been resolved.
fn work(edge: ActionEvent, source: &CapturedFrame) -> Option<Work> {
    match edge.action {
        Action::Attack if edge.pressed => Some(Work::Attack(Box::new(
            crate::melee::ordered::AttackAttempt::new(source.attack.clone()),
        ))),
        Action::Attack => Some(Work::Release),
        Action::Use => Some(Work::Use(Box::default())),
        Action::PickItem if edge.pressed => Some(Work::Pick(None)),
        action
            if edge.pressed && client_interaction::hotbar::slot_for_action(action, 0).is_some() =>
        {
            Some(Work::Selection(Box::default()))
        }
        _ => None,
    }
}

/// Drives at most one bounded burst, stopping at the first pending event or accepted fence.
pub(crate) fn drain(world: &mut World) {
    let snapshot = world
        .resource::<SemanticInputSnapshot>()
        .snapshot()
        .cloned();
    let Some(snapshot) = snapshot else {
        if let Some(mut runtime) = world.remove_resource::<InteractionRuntime>() {
            runtime.queue.cancel_pending();
            cancel_pending_owners(world);
            // Accepted carriers remain owned by their gameplay runtimes.
        }
        return;
    };
    let Some(ordered) = snapshot.ordered.as_ref() else {
        if let Some(mut runtime) = world.remove_resource::<InteractionRuntime>() {
            runtime.queue.cancel_pending();
            cancel_pending_owners(world);
        }
        return;
    };
    let mut runtime = world
        .remove_resource::<InteractionRuntime>()
        .unwrap_or_default();
    let movement = world.resource::<MovementTicker>();
    let authority = movement.interaction_authority_identity();
    let scope = InteractionScope {
        session: authority.0,
        position: authority.1,
        dimension: world
            .resource::<crate::runtime::world::ClientWorld>()
            .stream
            .as_ref()
            .map_or(0, |stream| stream.current_dimension()),
        input: snapshot.authority_generation,
    };
    let new_frame = runtime.queue.observe_frame(scope, snapshot.frame_sequence);
    if new_frame {
        runtime.ingested = 0;
        runtime.remaining = MAX_PENDING_ACTION_EVENTS;
    }
    let focused = world
        .query_filtered::<&Window, With<bevy::window::PrimaryWindow>>()
        .single(world)
        .is_ok_and(|window| window.focused)
        && !world.resource::<client_state::ScreenFocus>().menu_visible
        && !world.resource::<client_state::ScreenFocus>().ui_focused
        && !world.resource::<client_state::ScreenFocus>().absorbs_input;
    let player = world.resource::<client_state::PlayerRuntime>();
    let active = focused
        && player.facts.player_game_mode() != Some(protocol::PlayerGameMode::Spectator)
        && !world
            .resource::<crate::runtime::world::ClientWorld>()
            .respawn
            .input_held();
    let invalidation = runtime
        .queue
        .synchronize(scope, (new_frame && snapshot.authority_released) || !active);
    if invalidation != Invalidation::None {
        cancel_pending_owners(world);
        runtime.buttons = ActionButtons::default();
        if invalidation == Invalidation::World {
            world.resource_mut::<MeleeRuntime>().synchronize(authority);
            world
                .resource_mut::<BlockUseRuntime>()
                .synchronize(authority);
            world
                .resource_mut::<client_interaction::survival_mining::SurvivalMiningRuntime>()
                .synchronize(authority);
            world.resource_mut::<ItemUseRuntime>().cancel_for_respawn();
        }
    }
    let now =
        u64::try_from(world.resource::<Time<Real>>().elapsed().as_millis()).unwrap_or(u64::MAX);
    if new_frame || ordered.events().len() > runtime.ingested {
        if ordered.overflowed() {
            runtime.overflowed_batches = runtime.overflowed_batches.saturating_add(1);
            runtime.queue.quarantine();
            bevy::log::warn!(
                batches = runtime.overflowed_batches,
                "ordered action burst lost input; waiting for a neutral sample"
            );
            cancel_pending_owners(world);
            runtime.buttons = ActionButtons::default();
        } else {
            runtime.queue.observe_neutral(ordered.neutral());
            if active && !snapshot.authority_released && !runtime.queue.quarantined() {
                let tail = &ordered.events()[runtime.ingested..];
                let source = (!tail.is_empty()).then(|| capture(world, &snapshot));
                for edge in tail.iter().copied() {
                    let source = source.as_ref().expect("nonempty action batch");
                    if let Some(work) = work(edge, source) {
                        let event = Event {
                            edge,
                            source: source.clone(),
                            work,
                        };
                        if runtime.queue.push(event, now).is_err() {
                            runtime.overflowed_batches =
                                runtime.overflowed_batches.saturating_add(1);
                            bevy::log::warn!(
                                "pending interaction burst exceeded the action budget"
                            );
                            cancel_pending_owners(world);
                            runtime.buttons = ActionButtons::default();
                            break;
                        }
                    }
                }
            }
        }
        runtime.ingested = ordered.events().len();
    }
    world
        .run_system_cached(crate::item_use::ordered::flush_release)
        .expect("ordered release carrier");
    let stop_ready = world
        .run_system_cached_with(
            crate::block_use::ordered::fence,
            !active || snapshot.authority_released || runtime.queue.quarantined(),
        )
        .expect("accepted block stop carrier");
    let selection_ready = world
        .run_system_cached(crate::hotbar::ordered::flush_existing)
        .expect("older selection carrier");
    if !stop_ready || !selection_ready {
        world.insert_resource(runtime);
        return;
    }
    while runtime.remaining > 0 {
        let Some(mut entry) = runtime.queue.pop_front() else {
            break;
        };
        runtime.remaining -= 1;
        if entry.expired(now) {
            cancel_pending_owners(world);
            continue;
        }
        if world.resource::<MovementTicker>().has_held_release()
            || !world
                .resource::<MovementTicker>()
                .accepts_block_interactions()
        {
            runtime.queue.restore_front(entry);
            break;
        }
        if !entry.admitted {
            let player = world.resource::<client_state::PlayerRuntime>();
            if entry.value.source.selection_authority != player.inventory.selection_authority()
                || entry.value.source.policy != AdmissionPolicy::observe(player)
            {
                cancel_pending_owners(world);
                continue;
            }
        }
        let Event { edge, source, work } = entry.value;
        let input = InteractionInput {
            edge,
            context: source.context.clone(),
        };
        let step = dispatch(world, work, input);
        match step.progress {
            Progress::Complete => runtime.buttons = edge.buttons,
            Progress::Pending | Progress::Carrier => {
                if step.progress == Progress::Carrier {
                    runtime.buttons = edge.buttons;
                }
                entry.value = Event {
                    edge,
                    source,
                    work: step.attempt,
                };
                entry.admitted |= step.progress == Progress::Carrier;
                runtime.queue.restore_front(entry);
                break;
            }
        }
    }
    world.insert_resource(runtime);
}

/// Dispatches one event to its owner, without rerunning a frame producer or held maintenance.
fn dispatch(world: &mut World, work: Work, input: InteractionInput) -> Step<Work> {
    let edge = input.edge;
    if edge.action != Action::Use
        && !world
            .run_system_cached_with(
                crate::block_use::ordered::fence,
                edge.action == Action::Attack && edge.pressed,
            )
            .expect("older block stop carrier")
    {
        return Step::new(work, Progress::Pending);
    }
    match work {
        Work::Attack(attempt) => world
            .run_system_cached_with(crate::melee::ordered::step, (*attempt, input))
            .expect("ordered attack admission")
            .map(|attempt| Work::Attack(Box::new(attempt))),
        Work::Selection(attempt) => {
            if world
                .resource::<MovementTicker>()
                .between_ticks_sample()
                .is_none()
            {
                return Step::new(Work::Selection(attempt), Progress::Pending);
            }
            world
                .run_system_cached_with(crate::hotbar::ordered::step, (*attempt, edge.action))
                .expect("ordered selection admission")
                .map(|attempt| Work::Selection(Box::new(attempt)))
        }
        Work::Pick(attempt) => {
            if world
                .resource::<MovementTicker>()
                .between_ticks_sample()
                .is_none()
            {
                return Step::new(Work::Pick(attempt), Progress::Pending);
            }
            world
                .run_system_cached_with(crate::pick_block::step_ordered_pick, (attempt, input))
                .expect("ordered pick admission")
                .map(Work::Pick)
        }
        Work::Release => Step::new(Work::Release, Progress::Complete),
        Work::Use(mut attempt) => {
            if !attempt.block_done {
                let block = world
                    .run_system_cached_with(
                        crate::block_use::ordered::step,
                        (attempt.block, input.clone()),
                    )
                    .expect("ordered block admission");
                attempt.block = block.attempt;
                if attempt.block.cancelled() {
                    return Step::new(Work::Use(attempt), Progress::Complete);
                }
                if block.progress != Progress::Complete {
                    return Step::new(Work::Use(attempt), block.progress);
                }
                attempt.block_done = true;
            }
            let air = world
                .run_system_cached_with(crate::item_use::ordered::step, (attempt.air, input))
                .expect("ordered item admission");
            attempt.air = air.attempt;
            Step::new(Work::Use(attempt), air.progress)
        }
    }
}

#[cfg(test)]
mod tests;
