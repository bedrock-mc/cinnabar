//! Delayed rejection counts admit GPU work; they never authorize hiding terrain.

use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

use bevy::prelude::*;
use bevy::render::renderer::RenderDevice;

use super::model::{DECISION_CANDIDATE, DECISION_HIDDEN};
use super::visibility_admission::{PROBE_INTERVAL, VisibilityAdmission};

const IDLE: u8 = 0;
const RECORDING: u8 = 1;
const ENCODED: u8 = 2;
const PENDING: u8 = 3;
const MAPPED: u8 = 4;
const FAILED: u8 = 5;

/// View changes discard profitability, while delayed results only choose future work.
#[derive(Default)]
struct Admission {
    view: Option<Entity>,
    frame: u64,
    policy: Option<VisibilityAdmission>,
    first_frame: u64,
}

impl Admission {
    /// Unknown views start with fresh occlusion; unprofitable views draw conservatively.
    fn select(&mut self, view: Option<Entity>) -> bool {
        self.frame += 1;
        if self.view != view {
            self.view = view;
            self.policy = None;
            self.first_frame = self.frame;
        }
        view.is_some()
            && self
                .policy
                .as_mut()
                .is_none_or(|policy| policy.select_work(self.frame))
    }

    /// Feedback from a previous selection cannot change the current view's admission.
    fn observe(&mut self, view: Entity, frame: u64, candidates: usize, hidden: usize) {
        if self.view != Some(view) || frame < self.first_frame || candidates == 0 {
            return;
        }
        self.policy
            .get_or_insert_default()
            .observe(frame, candidates, hidden);
    }
}

/// One sparse asynchronous sample; a pending map skips sampling instead of waiting.
struct Sample {
    buffer: wgpu::Buffer,
    state: Arc<AtomicU8>,
    view: Entity,
    frame: u64,
    slots: u32,
}

#[derive(Resource, Default)]
pub(super) struct GpuCullFeedback {
    admission: Admission,
    sample: Option<Sample>,
    next_sample: u64,
}

impl GpuCullFeedback {
    /// Consumes completed samples for future submission choices without waiting.
    pub(super) fn select(&mut self, view: Option<Entity>) -> bool {
        let selected = self.admission.select(view);
        if let Some(sample) = &self.sample {
            match sample.state.load(Ordering::Acquire) {
                MAPPED => {
                    if let Ok(data) = sample
                        .buffer
                        .slice(..u64::from(sample.slots) * 4)
                        .get_mapped_range()
                    {
                        let (candidates, hidden) = rejection_counts(bytemuck::cast_slice(&data));
                        bevy::log::debug!(candidates, hidden, "terrain occlusion feedback");
                        self.admission
                            .observe(sample.view, sample.frame, candidates, hidden);
                    }
                    sample.buffer.unmap();
                    sample.state.store(IDLE, Ordering::Release);
                }
                FAILED => sample.state.store(IDLE, Ordering::Release),
                _ => {}
            }
        }
        selected
    }

    /// Reserves storage only for occasional selected frames, retaining it between samples.
    pub(super) fn prepare(&mut self, device: &RenderDevice, view: Entity, slots: u32) {
        if slots == 0 || self.admission.frame < self.next_sample {
            return;
        }
        if self
            .sample
            .as_ref()
            .is_some_and(|sample| sample.state.load(Ordering::Acquire) != IDLE)
        {
            return;
        }
        let bytes = u64::from(slots) * 4;
        if self
            .sample
            .as_ref()
            .is_none_or(|sample| sample.buffer.size() < bytes)
        {
            self.sample = Some(Sample {
                buffer: device.wgpu_device().create_buffer(&wgpu::BufferDescriptor {
                    label: Some("terrain rejection feedback"),
                    size: bytes.next_power_of_two(),
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                state: Arc::new(AtomicU8::new(IDLE)),
                view,
                frame: 0,
                slots: 0,
            });
        }
        let sample = self.sample.as_mut().expect("sample storage was ensured");
        sample.view = view;
        sample.frame = self.admission.frame;
        sample.slots = slots;
        sample.state.store(RECORDING, Ordering::Release);
        self.next_sample = self.admission.frame.saturating_add(PROBE_INTERVAL);
    }

    /// Copies decisions after the late cull; draw stream bits remain independent of feedback.
    pub(super) fn encode(&self, encoder: &mut wgpu::CommandEncoder, decisions: &wgpu::Buffer) {
        let Some(sample) = &self.sample else { return };
        if sample.state.load(Ordering::Acquire) == RECORDING {
            encoder.copy_buffer_to_buffer(
                decisions,
                0,
                &sample.buffer,
                0,
                u64::from(sample.slots) * 4,
            );
            sample.state.store(ENCODED, Ordering::Release);
        }
    }
}

/// Requests the map after render submission, without an additional poll or submission.
pub(super) fn submit_feedback(feedback: Res<GpuCullFeedback>) {
    let Some(sample) = &feedback.sample else {
        return;
    };
    match sample.state.load(Ordering::Acquire) {
        ENCODED => {
            sample.state.store(PENDING, Ordering::Release);
            let state = sample.state.clone();
            sample
                .buffer
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| {
                    state.store(
                        if result.is_ok() { MAPPED } else { FAILED },
                        Ordering::Release,
                    );
                });
        }
        RECORDING => sample.state.store(IDLE, Ordering::Release),
        _ => {}
    }
}

/// Counts only live, enabled, frustum-visible candidates and their current-depth rejections.
fn rejection_counts(decisions: &[u32]) -> (usize, usize) {
    decisions
        .iter()
        .fold((0, 0), |(candidates, hidden), decision| {
            let candidate = decision & DECISION_CANDIDATE != 0;
            (
                candidates + usize::from(candidate),
                hidden + usize::from(candidate && decision & DECISION_HIDDEN != 0),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::super::model::DECISION_DRAWS;
    use super::*;

    #[test]
    fn feedback_counts_candidates_independently_of_early_and_late_draws() {
        assert_eq!(
            rejection_counts(&[
                DECISION_CANDIDATE | DECISION_HIDDEN,
                DECISION_CANDIDATE | DECISION_DRAWS,
                DECISION_CANDIDATE,
                0,
            ]),
            (3, 1)
        );
    }

    #[test]
    fn low_rejection_skips_work_until_a_fresh_probe_and_view_changes_reset_it() {
        let mut world = World::new();
        let a = world.spawn_empty().id();
        let b = world.spawn_empty().id();
        let mut admission = Admission::default();
        assert!(admission.select(Some(a)));
        admission.observe(a, 1, 128, 2);
        assert!(admission.select(Some(a)));
        assert!(!admission.select(Some(a)));
        admission.frame += PROBE_INTERVAL;
        assert!(admission.select(Some(a)));
        admission.observe(a, admission.frame, 128, 100);
        assert!(admission.select(Some(a)));
        assert!(!admission.select(None));
        assert!(admission.select(Some(b)));
        admission.observe(a, admission.frame, 128, 0);
        assert!(admission.select(Some(b)));
        assert!(admission.select(Some(a)));
        admission.observe(a, 1, 128, 0);
        assert!(
            admission.select(Some(a)),
            "feedback from a previous view selection must be ignored"
        );
    }
}
