//! One tick's rig evaluation, concurrent yet identical to the serial order.
//!
//! Generations and the world budget are settled serially; rigs evaluate together only in a run
//! whose remaining world budget covers each rig's whole actor budget, so none can exhaust it.

use std::{
    cell::RefCell,
    sync::{Condvar, Mutex, PoisonError},
};

use super::*;

/// What a rig does this tick once its motion has advanced.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Step {
    /// Motion only; no pose this tick.
    Hold,
    GeometryOnly,
    Culled,
    Evaluate {
        pack: bool,
    },
}

struct TickJob<'a> {
    lifetime: ActorLifetimeId,
    state: &'a mut ActorRigState,
    actor: &'a ActorSnapshot,
    context: ActorTickContext,
    view_changed: bool,
    refresh_view: bool,
    step: Step,
    /// `None` for an evaluating rig means the world budget was already spent.
    outcome: Option<Evaluation>,
}

struct Evaluation {
    result: Result<EvaluatedState, EvalError>,
    used: usize,
    body_error: Option<EvalError>,
}

/// Catalogs a tick evaluates against.
#[derive(Clone, Copy)]
struct Catalogs<'a> {
    vanilla: (&'a RuntimeEntityAssets, &'a VariableLayout),
    pack: Option<(&'a RuntimeEntityAssets, &'a VariableLayout)>,
    tick: u64,
    exempt: Option<u64>,
    local_body_enabled: bool,
}

thread_local! {
    /// Operand stack reused by every expression this thread runs.
    static STACK: RefCell<Vec<evaluation::MolangValue>> = const { RefCell::new(Vec::new()) };
}

impl ActorAnimationStore {
    pub(super) fn evaluate_tick(
        &mut self,
        actors: &HashMap<u64, ActorSnapshot>,
        view: Option<&ActorAnimationView>,
        exempt: Option<u64>,
        step: PoseStep,
        context: impl Fn(&ActorSnapshot) -> ActorTickContext,
    ) {
        let PoseStep {
            evaluate,
            reset_motion_history,
            refresh_view,
        } = step;
        if !refresh_view {
            self.completed_tick = self.completed_tick.saturating_add(1);
        }
        let Some(assets) = self.assets.clone() else {
            return;
        };
        let parallel = self.parallel();
        let mut world_left = self.world_budget();
        let Self {
            layout,
            pack,
            rigs,
            runtime_to_lifetime,
            first_starved,
            completed_tick,
            next_reset_generation,
            next_rest_reset_generation,
            stats,
            local_body_enabled,
            ..
        } = self;
        let completed_tick = *completed_tick;
        let mut ordered: Vec<(ActorLifetimeId, &mut ActorRigState)> = if refresh_view {
            let local = exempt.and_then(|id| runtime_to_lifetime.get(&id).copied());
            rigs.iter_mut()
                .filter(|(lifetime, _)| Some(**lifetime) == local)
                .map(|(lifetime, state)| (*lifetime, state))
                .collect()
        } else {
            rigs.iter_mut()
                .map(|(lifetime, state)| (*lifetime, state))
                .collect()
        };
        // Start where the world budget ran out last tick so no actor starves every tick.
        if !refresh_view && let Some(start) = evaluate.then(|| first_starved.take()).flatten() {
            let first = ordered.partition_point(|(lifetime, _)| *lifetime < start);
            ordered.rotate_left(first);
        }
        let mut jobs = Vec::with_capacity(ordered.len());
        for (lifetime, state) in ordered {
            let Some(actor) = actors.get(&lifetime.runtime_id) else {
                continue;
            };
            // Observe ownership before any evaluation budget branch. A failed
            // animation cannot starve static publication for this or later actors.
            if actor.runtime_id == lifetime.runtime_id
                && actor.spawn_revision == lifetime.spawn_revision
                && runtime_to_lifetime.get(&lifetime.runtime_id) == Some(&lifetime)
            {
                if state.rest_reset_pending {
                    if let Some(next) = next_rest_reset_generation.checked_add(1) {
                        state.rest_reset_generation = *next_rest_reset_generation;
                        *next_rest_reset_generation = next;
                        state.rest_reset_pending = false;
                    } else {
                        state.rest_reset_generation = 0;
                    }
                }
                state.rest_completed_tick =
                    if state.rest_reset_generation != 0 && !state.rest_reset_pending {
                        completed_tick
                    } else {
                        0
                    };
            } else {
                state.rest_completed_tick = 0;
            }
            let context = context(actor);
            if reset_motion_history
                && skin::sync_skin(state, context.skin_geometry.as_ref(), &assets)
            {
                stats.invalid_skin_geometries = stats.invalid_skin_geometries.saturating_add(1);
            }
            if !refresh_view {
                advance_motion(state, actor, &context, reset_motion_history);
            }
            let view_changed = state
                .view_context
                .is_some_and(|old| old != context.is_local_first_person);
            let step = if !evaluate {
                Step::Hold
            } else if state.fallback == EntityRigFallback::GeometryOnly {
                Step::GeometryOnly
            } else {
                let catalog = if state.pack {
                    pack.as_ref().map(|pack| &*pack.assets)
                } else {
                    Some(&*assets)
                };
                match catalog {
                    None => Step::Hold,
                    Some(catalog) if culled(view, exempt, state, actor, catalog) => Step::Culled,
                    Some(_) => Step::Evaluate { pack: state.pack },
                }
            };
            jobs.push(TickJob {
                lifetime,
                state,
                actor,
                context,
                view_changed,
                refresh_view,
                step,
                outcome: None,
            });
        }
        let catalogs = OwnedCatalogs {
            vanilla: (Arc::clone(&assets), Arc::clone(layout)),
            pack: pack
                .as_ref()
                .map(|pack| (Arc::clone(&pack.assets), Arc::clone(&pack.layout))),
            tick: completed_tick,
            exempt,
            local_body_enabled: *local_body_enabled,
        };
        let mut ledger = Ledger {
            tick: completed_tick,
            advance_history: !refresh_view,
            next_reset_generation,
            stats,
            starved: None,
        };
        let mut next = 0;
        while next < jobs.len() {
            // Each rig spends at most its actor budget, so this many fit before the world runs out.
            let capacity = if parallel {
                world_left / MAX_MOLANG_OPS_PER_ACTOR_TICK
            } else {
                0
            };
            if capacity < 2 {
                let job = &mut jobs[next];
                if job.evaluates() && world_left > 0 {
                    job.outcome = Some(evaluate_job(job, catalogs.view(), &mut world_left));
                }
                ledger.apply(job);
                next += 1;
                continue;
            }
            let (mut end, mut evaluating) = (next, 0);
            while end < jobs.len() && (evaluating < capacity || !jobs[end].evaluates()) {
                evaluating += usize::from(jobs[end].evaluates());
                end += 1;
            }
            evaluate_concurrently(&mut jobs[next..end], evaluating, catalogs.clone());
            for job in &mut jobs[next..end] {
                if let Some(outcome) = &job.outcome {
                    world_left = world_left.saturating_sub(outcome.used);
                }
                ledger.apply(job);
            }
            next = end;
        }
        let starved = ledger.starved;
        if evaluate && !refresh_view {
            *first_starved = starved;
        }
    }

    #[cfg(not(test))]
    const fn parallel(&self) -> bool {
        true
    }

    #[cfg(not(test))]
    const fn world_budget(&self) -> usize {
        MAX_MOLANG_OPS_PER_WORLD_TICK
    }

    #[cfg(test)]
    fn parallel(&self) -> bool {
        !self.schedule.serial
    }

    #[cfg(test)]
    fn world_budget(&self) -> usize {
        self.schedule.world_budget
    }
}

/// Test-only overrides proving the parallel schedule matches the serial one.
#[cfg(test)]
#[derive(Debug)]
pub(super) struct TestSchedule {
    pub(super) serial: bool,
    pub(super) world_budget: usize,
}

#[cfg(test)]
impl Default for TestSchedule {
    fn default() -> Self {
        Self {
            serial: false,
            world_budget: MAX_MOLANG_OPS_PER_WORLD_TICK,
        }
    }
}

impl TickJob<'_> {
    const fn evaluates(&self) -> bool {
        matches!(self.step, Step::Evaluate { .. })
    }
}

/// Whether the rig sits outside the animation view on both of its last two positions.
fn culled(
    view: Option<&ActorAnimationView>,
    exempt: Option<u64>,
    state: &ActorRigState,
    actor: &ActorSnapshot,
    assets: &RuntimeEntityAssets,
) -> bool {
    let Some(view) = view.filter(|_| exempt != Some(actor.runtime_id)) else {
        return false;
    };
    let scale = model_scale(state, assets) * actor.render_scale();
    let player = matches!(actor.kind, ActorKind::Player { .. });
    let bounds = state
        .skin_skeleton()
        .and_then(|skin| skin.geometry.visible_bounds)
        .unwrap_or_default();
    !view.admits(actor.position, scale, player, bounds)
        && !view.admits(actor.previous_pose.position, scale, player, bounds)
}

/// Rigs each pool helper must have to itself to repay waking it.
const RIGS_PER_HELPER: usize = 8;
/// Helpers beside the calling thread; the world pool already holds most cores.
const MAX_HELPERS: usize = 3;

/// Catalogs a helper holds past the caller's borrow.
#[derive(Clone)]
struct OwnedCatalogs {
    vanilla: (Arc<RuntimeEntityAssets>, Arc<VariableLayout>),
    pack: Option<(Arc<RuntimeEntityAssets>, Arc<VariableLayout>)>,
    tick: u64,
    exempt: Option<u64>,
    local_body_enabled: bool,
}

impl OwnedCatalogs {
    fn view(&self) -> Catalogs<'_> {
        Catalogs {
            vanilla: (&self.vanilla.0, &self.vanilla.1),
            pack: self
                .pack
                .as_ref()
                .map(|(assets, layout)| (&**assets, &**layout)),
            tick: self.tick,
            exempt: self.exempt,
            local_body_enabled: self.local_body_enabled,
        }
    }
}

/// Rigs of one run, claimed one at a time by the caller and any helper that wakes in time.
struct Board {
    claims: Mutex<Claims>,
    settled: Condvar,
    jobs: JobsPtr,
    len: usize,
    catalogs: OwnedCatalogs,
}

struct Claims {
    next: usize,
    /// Cleared once the caller stops waiting; a later helper then touches nothing.
    open: bool,
    /// Rigs claimed and not yet finished.
    busy: usize,
    panic: Option<Box<dyn std::any::Any + Send>>,
}

struct JobsPtr(*mut TickJob<'static>);

// SAFETY: the pointer is dereferenced only for a claimed index while the board is open, and the
// caller keeps the jobs alive and untouched until every claim settles (`evaluate_concurrently`).
unsafe impl Send for JobsPtr {}
// SAFETY: as above; distinct claims never alias.
unsafe impl Sync for JobsPtr {}

impl Board {
    fn lock(&self) -> std::sync::MutexGuard<'_, Claims> {
        self.claims.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Evaluates claimed rigs until none are left or the caller has stopped waiting.
    fn help(&self) {
        loop {
            let index = {
                let mut claims = self.lock();
                if !claims.open || claims.next == self.len {
                    return;
                }
                claims.next += 1;
                claims.busy += 1;
                claims.next - 1
            };
            // SAFETY: `index` was claimed exactly once while the board was open, so no other
            // thread holds this job, and the caller waits for `busy` to reach zero before the
            // borrow behind `jobs` ends.
            let job = unsafe { &mut *self.jobs.0.add(index) };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if job.evaluates() {
                    let mut own = MAX_MOLANG_OPS_PER_ACTOR_TICK;
                    job.outcome = Some(evaluate_job(job, self.catalogs.view(), &mut own));
                }
            }));
            let mut claims = self.lock();
            claims.busy -= 1;
            if let Err(panic) = result {
                claims.panic.get_or_insert(panic);
            }
            if claims.busy == 0 {
                self.settled.notify_all();
            }
        }
    }
}

/// Evaluates a run's rigs on the calling thread while pool threads claim rigs beside it. The
/// caller never waits for a helper that has not claimed a rig, so a busy pool costs it nothing.
fn evaluate_concurrently(jobs: &mut [TickJob<'_>], evaluating: usize, catalogs: OwnedCatalogs) {
    let helpers = (evaluating / RIGS_PER_HELPER)
        .min(MAX_HELPERS)
        .min(rayon::current_num_threads().saturating_sub(1));
    let board = Arc::new(Board {
        claims: Mutex::new(Claims {
            next: 0,
            open: true,
            busy: 0,
            panic: None,
        }),
        settled: Condvar::new(),
        jobs: JobsPtr(jobs.as_mut_ptr().cast()),
        len: jobs.len(),
        catalogs,
    });
    for _ in 0..helpers {
        let board = Arc::clone(&board);
        rayon::spawn(move || board.help());
    }
    board.help();
    let mut claims = board.lock();
    claims.open = false;
    while claims.busy > 0 {
        claims = board
            .settled
            .wait(claims)
            .unwrap_or_else(PoisonError::into_inner);
    }
    if let Some(panic) = claims.panic.take() {
        drop(claims);
        std::panic::resume_unwind(panic);
    }
}

/// Evaluates one rig, touching only its own state.
fn evaluate_job(
    job: &mut TickJob<'_>,
    catalogs: Catalogs<'_>,
    world_left: &mut usize,
) -> Evaluation {
    let Step::Evaluate { pack } = job.step else {
        unreachable!("only evaluating rigs run");
    };
    let Some((assets, layout)) = (if pack {
        catalogs.pack
    } else {
        Some(catalogs.vanilla)
    }) else {
        unreachable!("an evaluating rig's catalog was present when it was scheduled");
    };
    let (state, actor, context) = (&mut *job.state, job.actor, &job.context);
    let mut budget = EvalBudget {
        actor_left: MAX_MOLANG_OPS_PER_ACTOR_TICK,
        world_left,
        work_left: MAX_RUNTIME_POSE_WORK_PER_ACTOR_TICK,
        transitions_left: MAX_CONTROLLER_TRANSITIONS_PER_TICK,
        used: 0,
        stack: STACK.with_borrow_mut(std::mem::take),
    };
    render::cache_layer_skeletons(assets, state);
    if job.refresh_view {
        geometry::reselect_geometry_replay(
            assets,
            layout,
            state,
            actor,
            context,
            &mut budget,
            catalogs.tick,
        );
    } else {
        geometry::reselect_geometry(assets, layout, state, actor, context, &mut budget);
    }
    state.refresh_skin_drivers();
    let result = evaluate_state(
        assets,
        layout,
        state,
        actor,
        context,
        catalogs.tick,
        &mut budget,
        !job.refresh_view,
        None,
    );
    if catalogs.exempt == Some(actor.runtime_id) {
        let ui_context = ActorTickContext {
            is_local_first_person: false,
            is_in_ui: true,
            ..context.clone()
        };
        hud::evaluate(
            assets,
            layout,
            state,
            actor,
            &ui_context,
            catalogs.tick,
            &mut budget,
            !job.refresh_view,
        );
    } else {
        state.ui_pose = None;
        state.ui_animation = None;
    }
    let body_error = if catalogs.local_body_enabled
        && catalogs.exempt == Some(actor.runtime_id)
        && context.is_local_first_person
    {
        body::evaluate(assets, layout, state, actor, context, catalogs.tick, &mut budget).err()
    } else {
        state.world_body = None;
        None
    };
    let used = budget.used;
    STACK.set(budget.stack);
    Evaluation {
        result,
        used,
        body_error,
    }
}

/// Store-wide counters a tick advances, always in the serial order.
struct Ledger<'a> {
    tick: u64,
    advance_history: bool,
    next_reset_generation: &'a mut u64,
    stats: &'a mut ActorAnimationStats,
    starved: Option<ActorLifetimeId>,
}

impl Ledger<'_> {
    fn take_reset_generation(&mut self) -> u64 {
        let generation = *self.next_reset_generation;
        *self.next_reset_generation = generation.saturating_add(1);
        generation
    }

    fn freeze(&mut self) {
        self.stats.frozen_actors = self.stats.frozen_actors.saturating_add(1);
    }

    fn starve(&mut self, lifetime: ActorLifetimeId) {
        self.stats.world_budget_exhaustions = self.stats.world_budget_exhaustions.saturating_add(1);
        self.freeze();
        self.starved.get_or_insert(lifetime);
    }

    fn apply(&mut self, job: &mut TickJob<'_>) {
        let state = &mut *job.state;
        match job.step {
            Step::Hold => {}
            Step::GeometryOnly => {
                if self.advance_history {
                    state.previous.clone_from(&state.current);
                }
                if state.reset_pending {
                    state.reset_pending = false;
                    state.reset_generation = self.take_reset_generation();
                    state.animation_epoch = self.tick;
                }
                state.completed_tick = self.tick;
            }
            Step::Culled => {
                state.culled = true;
                if self.advance_history {
                    state.previous.clone_from(&state.current);
                }
                state.completed_tick = self.tick;
            }
            Step::Evaluate { .. } => {
                let Some(Evaluation {
                    result,
                    used,
                    body_error,
                }) = job.outcome.take()
                else {
                    self.starve(job.lifetime);
                    // A frozen tick holds the pose instead of replaying the last change.
                    if self.advance_history {
                        state.previous.clone_from(&state.current);
                    }
                    return;
                };
                self.stats.evaluated_molang_ops =
                    self.stats.evaluated_molang_ops.saturating_add(used as u64);
                if let Some(error) = body_error {
                    match error {
                        EvalError::ActorBudget => {
                            self.stats.actor_budget_exhaustions =
                                self.stats.actor_budget_exhaustions.saturating_add(1);
                        }
                        EvalError::WorldBudget => {
                            self.stats.world_budget_exhaustions =
                                self.stats.world_budget_exhaustions.saturating_add(1);
                        }
                        EvalError::Invalid => {}
                    }
                    self.freeze();
                }
                match result {
                    Ok(evaluated) => self.commit(job, evaluated),
                    Err(error) => {
                        match error {
                            EvalError::ActorBudget => {
                                self.stats.actor_budget_exhaustions =
                                    self.stats.actor_budget_exhaustions.saturating_add(1);
                                self.freeze();
                            }
                            EvalError::WorldBudget => self.starve(job.lifetime),
                            EvalError::Invalid => self.freeze(),
                        }
                        if self.advance_history {
                            job.state.previous.clone_from(&job.state.current);
                        }
                    }
                }
            }
        }
    }

    fn commit(&mut self, job: &mut TickJob<'_>, mut evaluated: EvaluatedState) {
        let state = &mut *job.state;
        let view_changed = job.view_changed;
        // A rig back in view starts from its new pose, not the one it held.
        let resumed = std::mem::take(&mut state.culled);
        replay::Replay::commit(
            state,
            &mut evaluated,
            &job.context,
            self.tick,
            job.refresh_view,
            job.context.is_local,
        );
        state.scale = evaluated.scale;
        state.render_frame = evaluated.render_frame;
        let restart = state.reset_pending || resumed || view_changed;
        skin_layers::carry(
            &state.skin_layers,
            &mut evaluated.skin_layers,
            restart,
            self.advance_history,
        );
        state.skin_layers = evaluated.skin_layers;
        if let Some(mut render) = evaluated.render {
            render::carry_layer_poses(&state.render, &mut render, restart, self.advance_history);
            state.render = render;
        }
        state.initialized = true;
        if state.reset_pending {
            state.previous.clone_from(&evaluated.pose);
            state.current = evaluated.pose;
            state.reset_pending = false;
            state.reset_generation = self.take_reset_generation();
            state.animation_epoch = self.tick;
        } else if resumed || view_changed {
            state.previous.clone_from(&evaluated.pose);
            state.current = evaluated.pose;
        } else if self.advance_history {
            state.previous = std::mem::replace(&mut state.current, evaluated.pose);
        } else {
            state.current = evaluated.pose;
        }
        if view_changed {
            state.reset_generation = self.take_reset_generation();
        }
        state.view_context = Some(job.context.is_local_first_person);
        state.completed_tick = self.tick;
    }
}

#[cfg(test)]
mod tests;
