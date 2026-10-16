//! Previous-tick evaluation for returning rigs, with serial budget and commit order.

use super::*;

/// Temporarily removes only the current input and borrows the preceding elapsed interval.
struct WarmContext<'job, 'state> {
    job: &'job mut TickJob<'state>,
    latest: Option<ActorTickInput>,
    elapsed: Option<u32>,
}

impl<'job, 'state> WarmContext<'job, 'state> {
    /// Restores transient inputs on normal return and unwinding, without rolling back rig effects.
    fn new(job: &'job mut TickJob<'state>) -> Self {
        let elapsed = job.context.animation_elapsed_ticks;
        let latest = job.state.history.pop_back();
        job.context.animation_elapsed_ticks = warm_elapsed(elapsed);
        Self {
            job,
            latest,
            elapsed,
        }
    }
}

impl Drop for WarmContext<'_, '_> {
    /// Restores the borrowed sample and original elapsed value before releasing the job.
    fn drop(&mut self) {
        self.job.state.history.extend(self.latest.take());
        self.job.context.animation_elapsed_ticks = self.elapsed;
    }
}

/// The previous evaluation owns all but the last tick of the original interval.
fn warm_elapsed(elapsed: Option<u32>) -> Option<u32> {
    Some(elapsed.unwrap_or(1).saturating_sub(1))
}

/// Evaluates T-1 with transient inputs restored before an outcome reaches the ledger.
pub(super) fn evaluate(
    job: &mut TickJob<'_>,
    catalogs: Catalogs<'_>,
    left: &mut usize,
) -> Evaluation {
    let guard = WarmContext::new(job);
    evaluate_job(
        guard.job,
        Catalogs {
            tick: catalogs.tick.saturating_sub(1),
            ..catalogs
        },
        left,
    )
}

/// Warms returning rigs before current evaluation using only the existing pool and helper limits.
pub(super) fn run(
    jobs: &mut [TickJob<'_>],
    catalogs: &OwnedCatalogs,
    ledger: &mut Ledger<'_>,
    world_left: &mut usize,
    parallel: bool,
) -> Vec<usize> {
    #[cfg(test)]
    if !parallel {
        return tests::serial_reference(jobs, catalogs, ledger, world_left);
    }
    let available = if parallel {
        rayon::current_num_threads().saturating_sub(1)
    } else {
        0
    };
    run_with(
        jobs,
        catalogs,
        ledger,
        world_left,
        available,
        evaluate_with_helpers,
    )
}

/// Selects safe prefixes and settles their actual costs before considering the next prefix.
fn run_with(
    jobs: &mut [TickJob<'_>],
    catalogs: &OwnedCatalogs,
    ledger: &mut Ledger<'_>,
    world_left: &mut usize,
    available: usize,
    mut dispatch: impl FnMut(&mut [TickJob<'_>], OwnedCatalogs, Pass, usize),
) -> Vec<usize> {
    for job in jobs.iter_mut() {
        job.warm_candidate = job.evaluates() && job.state.culled && job.state.history.len() > 1;
    }
    let mut warm_left = *world_left / RESUME_WARMUP_SHARE;
    let mut failed = Vec::new();
    let mut credits = MAX_HELPERS;
    let mut next = 0;
    while next < jobs.len() {
        if !jobs[next].warm_candidate {
            next += 1;
            continue;
        }
        let capacity = warm_left / MAX_MOLANG_OPS_PER_ACTOR_TICK;
        let (mut end, mut count) = (next, 0);
        if credits.min(available) > 0 && capacity >= 2 {
            while end < jobs.len() && count < capacity {
                count += usize::from(jobs[end].warm_candidate);
                end += 1;
            }
        }
        let helpers = helper_count(count, credits.min(available));
        if helpers == 0 {
            // The partial tail retains its shared allowance, including zero-budget skips.
            let job = &mut jobs[next];
            if warm_left > 0 {
                job.outcome = Some(evaluate(job, catalogs.view(), &mut warm_left));
            }
            if apply(job, ledger, world_left) {
                failed.push(next);
            }
            next += 1;
            continue;
        }
        credits -= helpers;
        // With L >= count * B, every serial allowance is at least B. Private full caps
        // therefore preserve actor-first errors; only actual usage reduces the next allowance.
        dispatch(
            &mut jobs[next..end],
            catalogs.clone(),
            Pass::PreviousWarmup,
            helpers,
        );
        for (offset, job) in jobs[next..end].iter_mut().enumerate() {
            if !job.warm_candidate {
                continue;
            }
            if let Some(outcome) = &job.outcome {
                warm_left = warm_left.saturating_sub(outcome.used);
            }
            if apply(job, ledger, world_left) {
                failed.push(next + offset);
            }
        }
        next = end;
    }
    failed
}

/// Applies a warm outcome with T's ledger and the warm elapsed interval, then installs the tail.
/// Failures count only their actual operations and never leak into current-pass error handling.
fn apply(job: &mut TickJob<'_>, ledger: &mut Ledger<'_>, world_left: &mut usize) -> bool {
    let elapsed = job.context.animation_elapsed_ticks;
    job.context.animation_elapsed_ticks = warm_elapsed(elapsed);
    let mut failed = false;
    if let Some(outcome) = job.outcome.take() {
        *world_left = world_left.saturating_sub(outcome.used);
        if outcome.result.is_ok() {
            job.outcome = Some(outcome);
            ledger.apply(job);
        } else {
            ledger.stats.evaluated_molang_ops = ledger
                .stats
                .evaluated_molang_ops
                .saturating_add(outcome.used as u64);
            failed = true;
        }
    }
    job.context.animation_elapsed_ticks = elapsed.map(|ticks| ticks.min(1));
    failed
}

#[cfg(test)]
mod tests;
