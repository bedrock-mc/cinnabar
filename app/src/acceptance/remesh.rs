use std::{
    collections::BTreeSet,
    sync::Arc,
    time::{Duration, Instant},
};

use client_world::ViewCohortStatus;
use render::{PresentedFrameAck, RenderViewCohort, TargetRenderExpectation};
use world::SubChunkKey;

use super::teleport::{
    FullViewTeleportCompletion, TeleportReadySnapshot, presented_ack_matches, render_view_cohort,
};

#[derive(Debug, Clone)]
pub(crate) struct FullViewRemeshPresentedCandidate {
    pub(crate) expectation: TargetRenderExpectation,
    pub(crate) first_frame: Option<PresentedFrameAck>,
}

#[derive(Debug, Clone)]
pub(crate) struct PendingFullViewRemesh {
    pub(crate) manifest: client_world::ForcedRemeshManifest,
    pub(crate) cohort: ViewCohortStatus,
    pub(crate) source_cohort: Option<RenderViewCohort>,
    pub(crate) binding_manifest: Arc<[(SubChunkKey, u64)]>,
    pub(crate) binding_view_generation: u64,
    pub(crate) started_frame_count: u64,
    pub(crate) candidate: Option<FullViewRemeshPresentedCandidate>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FullViewRemeshCompletion {
    pub(crate) settle_latency: Duration,
    pub(crate) render_ready_latency: Duration,
    pub(crate) first_present_return_latency: Duration,
    pub(crate) first_gpu_completion_latency: Duration,
    pub(crate) stable_present_return_latency: Duration,
    pub(crate) stable_gpu_completion_latency: Duration,
    pub(crate) view_generation: u64,
    pub(crate) expectation: TargetRenderExpectation,
    pub(crate) first_frame: PresentedFrameAck,
    pub(crate) stable_frame: PresentedFrameAck,
    pub(crate) frame_count: u64,
}

#[derive(Debug, Default)]
pub(crate) struct FullViewRemeshTracker {
    pub(crate) pending: Option<PendingFullViewRemesh>,
    pub(crate) completed: Option<FullViewRemeshCompletion>,
    pub(crate) invalidated: bool,
}

impl FullViewRemeshTracker {
    pub(crate) fn start(
        &mut self,
        binding: Option<&FullViewTeleportCompletion>,
        cohort: ViewCohortStatus,
        manifest: client_world::ForcedRemeshManifest,
        frame_count: u64,
    ) -> bool {
        let Some(binding) = binding else {
            return false;
        };
        let manifest_keys = manifest
            .entries
            .iter()
            .map(|(key, _)| *key)
            .collect::<BTreeSet<_>>();
        let binding_keys = binding
            .expectation
            .manifest
            .iter()
            .map(|(key, _)| *key)
            .collect::<BTreeSet<_>>();
        if manifest.is_empty()
            || binding.expectation.manifest.is_empty()
            || manifest.started_at < binding.stable_frame.gpu_completed_at
            || !cohort.is_exact()
            || render_view_cohort(cohort.target) != binding.expectation.cohort
            || manifest_keys != binding_keys
            || self.pending.is_some()
            || self.completed.is_some()
            || self.invalidated
        {
            return false;
        }
        self.pending = Some(PendingFullViewRemesh {
            manifest,
            cohort,
            source_cohort: binding.expectation.source_cohort,
            binding_manifest: Arc::clone(&binding.expectation.manifest),
            binding_view_generation: binding.view_generation,
            started_frame_count: frame_count,
            candidate: None,
        });
        true
    }

    pub(crate) fn reconcile_presented_expectation(
        &mut self,
        snapshot: TeleportReadySnapshot,
        manifest_state: client_world::ForcedRemeshManifestState,
        proposed: Option<TargetRenderExpectation>,
        now: Instant,
        _frame_count: u64,
    ) -> Option<TargetRenderExpectation> {
        if manifest_state == client_world::ForcedRemeshManifestState::Invalid {
            self.invalidate();
            return None;
        }
        let pending = self.pending.as_ref()?;
        if snapshot.cohort != Some(pending.cohort) {
            self.invalidate();
            return None;
        }
        if manifest_state == client_world::ForcedRemeshManifestState::Pending
            || !snapshot.is_ready()
        {
            self.pending
                .as_mut()
                .expect("validated forced remesh remains pending")
                .candidate = None;
            return None;
        }
        let Some(mut proposed) = proposed else {
            self.invalidate();
            return None;
        };

        if let Some(candidate) = &pending.candidate {
            if proposed.cohort != candidate.expectation.cohort
                || proposed.source_cohort != candidate.expectation.source_cohort
                || proposed.manifest != candidate.expectation.manifest
            {
                self.invalidate();
                return None;
            }
            return Some(candidate.expectation.clone());
        }

        let binding_keys = pending
            .binding_manifest
            .iter()
            .map(|(key, _)| *key)
            .collect::<BTreeSet<_>>();
        let proposed_keys = proposed
            .manifest
            .iter()
            .map(|(key, _)| *key)
            .collect::<BTreeSet<_>>();
        let proposal_is_forced = !proposed.manifest.is_empty()
            && proposed.cohort == render_view_cohort(pending.cohort.target)
            && proposed.source_cohort == pending.source_cohort
            && proposed.manifest.len() == pending.binding_manifest.len()
            && proposed_keys == binding_keys
            && proposed.manifest != pending.binding_manifest
            && proposed
                .manifest
                .iter()
                .all(|entry| pending.manifest.entries.contains(entry));
        if !proposal_is_forced {
            self.invalidate();
            return None;
        }

        proposed.view_generation = pending.binding_view_generation.wrapping_add(1).max(1);
        proposed.render_ready_at = now;
        let expectation = proposed.clone();
        self.pending
            .as_mut()
            .expect("validated forced remesh remains pending")
            .candidate = Some(FullViewRemeshPresentedCandidate {
            expectation,
            first_frame: None,
        });
        Some(proposed)
    }

    pub(crate) fn observe_presented_frame(
        &mut self,
        acknowledgement: PresentedFrameAck,
        frame_count: u64,
    ) -> Option<FullViewRemeshCompletion> {
        let invalid_current_evidence = {
            let pending = self.pending.as_ref()?;
            let candidate = pending.candidate.as_ref()?;
            acknowledgement.cohort == candidate.expectation.cohort
                && acknowledgement.view_generation == candidate.expectation.view_generation
                && acknowledgement.render_ready_at == candidate.expectation.render_ready_at
                && !presented_ack_matches(
                    pending.manifest.started_at,
                    &candidate.expectation,
                    &acknowledgement,
                )
        };
        if invalid_current_evidence {
            self.invalidate();
            return None;
        }
        let completion = {
            let pending = self.pending.as_mut()?;
            let candidate = pending.candidate.as_mut()?;
            if !presented_ack_matches(
                pending.manifest.started_at,
                &candidate.expectation,
                &acknowledgement,
            ) {
                candidate.first_frame = None;
                return None;
            }
            let Some(first) = candidate.first_frame.take() else {
                candidate.first_frame = Some(acknowledgement);
                return None;
            };
            if !first.forms_stable_exact_pair_with(&acknowledgement)
                || first.present_returned_at > acknowledgement.present_returned_at
            {
                candidate.first_frame = Some(acknowledgement);
                return None;
            }

            let started = pending.manifest.started_at;
            Some(FullViewRemeshCompletion {
                settle_latency: acknowledgement
                    .gpu_completed_at
                    .checked_duration_since(started)?,
                render_ready_latency: candidate
                    .expectation
                    .render_ready_at
                    .checked_duration_since(started)?,
                first_present_return_latency: first
                    .present_returned_at
                    .checked_duration_since(started)?,
                first_gpu_completion_latency: first
                    .gpu_completed_at
                    .checked_duration_since(started)?,
                stable_present_return_latency: acknowledgement
                    .present_returned_at
                    .checked_duration_since(started)?,
                stable_gpu_completion_latency: acknowledgement
                    .gpu_completed_at
                    .checked_duration_since(started)?,
                view_generation: candidate.expectation.view_generation,
                expectation: candidate.expectation.clone(),
                first_frame: first,
                stable_frame: acknowledgement,
                frame_count: frame_count
                    .saturating_sub(pending.started_frame_count)
                    .saturating_add(1)
                    .max(2),
            })
        };
        if let Some(completion) = &completion {
            self.completed = Some(completion.clone());
            self.pending = None;
        }
        completion
    }

    pub(crate) fn invalidate(&mut self) {
        self.pending = None;
        self.invalidated = true;
    }

    #[cfg(test)]
    pub(crate) fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    #[cfg(test)]
    pub(crate) const fn is_invalidated(&self) -> bool {
        self.invalidated
    }
}
