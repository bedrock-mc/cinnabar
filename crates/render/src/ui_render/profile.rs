//! Opt-in submitted UI work counters accompany the owned GPU pass timings.

use crate::RuntimeStage;
use bevy::{
    app::SubApp,
    prelude::*,
    render::{Render, RenderSystems, renderer::render_system},
};
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

/// Upload categories count submitted transfers through queue writes or staging.
#[derive(Clone, Copy)]
pub(crate) enum UploadKind {
    Geometry,
    Texture,
    Viewport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LayerTarget {
    extent: [u32; 2],
    format: wgpu::TextureFormat,
    samples: u32,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct Interval {
    frames: u64,
    redraws: u64,
    reuses: u64,
    partial_redraws: u64,
    partial_pixels: u64,
    target: Option<LayerTarget>,
    passes: [u64; RuntimeStage::GPU_UI.len()],
    draws: [u64; RuntimeStage::GPU_UI.len()],
    indices: [u64; RuntimeStage::GPU_UI.len()],
    upload_calls: [u64; 3],
    upload_bytes: [u64; 3],
}

impl Interval {
    /// Allocates report fields only once per reporting interval, never during submission.
    fn json(&self, elapsed: Duration) -> serde_json::Value {
        let stages: serde_json::Map<_, _> = RuntimeStage::GPU_UI
            .into_iter()
            .enumerate()
            .map(|(index, stage)| {
                (
                    stage.name().to_owned(),
                    serde_json::json!({
                        "passes": self.passes[index],
                        "draws": self.draws[index],
                        "indices": self.indices[index],
                    }),
                )
            })
            .collect();
        let uploads: serde_json::Map<_, _> = ["geometry", "texture", "viewport"]
            .into_iter()
            .enumerate()
            .map(|(index, name)| {
                (
                    name.to_owned(),
                    serde_json::json!({
                        "calls": self.upload_calls[index],
                        "bytes": self.upload_bytes[index],
                    }),
                )
            })
            .collect();
        serde_json::json!({
            "interval_ms": elapsed.as_secs_f64() * 1000.0,
            "frames": self.frames,
            "layer_redraws": self.redraws,
            "layer_reuses": self.reuses,
            "partial_redraws": self.partial_redraws,
            "partial_pixels": self.partial_pixels,
            "last_layer_extent": self.target.map(|target| target.extent),
            "last_layer_format": self.target.map(|target| format!("{:?}", target.format)),
            "last_layer_samples": self.target.map(|target| target.samples),
            "stages": stages,
            "uploads": uploads,
        })
    }
}

/// Present only with UI diagnostics enabled; graph nodes share its bounded counters.
#[derive(Resource)]
pub(crate) struct UiProfile {
    pub(super) baseline_replay: bool,
    started: Instant,
    interval: Mutex<Interval>,
}

impl Default for UiProfile {
    fn default() -> Self {
        Self {
            baseline_replay: std::env::var_os("RUST_MCBE_GPU_UI_BASELINE")
                .is_some_and(|value| value == "1"),
            started: Instant::now(),
            interval: Mutex::default(),
        }
    }
}

impl UiProfile {
    /// Gives tests an explicit replay policy independent of process settings.
    #[cfg(test)]
    pub(super) fn with_baseline_replay(baseline_replay: bool) -> Self {
        Self {
            baseline_replay,
            ..Self::default()
        }
    }

    /// Records the bounded area restored by a submitted partial replay.
    pub(crate) fn record_damage(&self, rect: render_model::UiScissor) {
        let mut interval = self.interval.lock().expect("UI profile lock");
        interval.partial_redraws += 1;
        interval.partial_pixels += u64::from(rect.width) * u64::from(rect.height);
    }

    /// Counts the cache decision and records the target used by this layer.
    pub(crate) fn record_layer(
        &self,
        redrawn: bool,
        extent: [u32; 2],
        format: wgpu::TextureFormat,
        samples: u32,
    ) {
        let mut interval = self.interval.lock().expect("UI profile lock");
        interval.redraws += u64::from(redrawn);
        interval.reuses += u64::from(!redrawn);
        interval.target = Some(LayerTarget {
            extent,
            format,
            samples,
        });
    }

    /// Counts an encoded pass, including a clear pass with no draw commands.
    pub(crate) fn record_pass(&self, stage: RuntimeStage) {
        if let Some(index) = RuntimeStage::GPU_UI
            .iter()
            .position(|value| *value == stage)
        {
            self.interval.lock().expect("UI profile lock").passes[index] += 1;
        }
    }

    /// Counts an issued draw and its submitted indices; non-indexed draws pass zero.
    pub(crate) fn record_draw(&self, stage: RuntimeStage, index_count: u32) {
        if let Some(index) = RuntimeStage::GPU_UI
            .iter()
            .position(|value| *value == stage)
        {
            let mut interval = self.interval.lock().expect("UI profile lock");
            interval.draws[index] += 1;
            interval.indices[index] += u64::from(index_count);
        }
    }

    /// Counts one nonempty upload and the bytes it transfers.
    pub(crate) fn record_upload(&self, kind: UploadKind, bytes: u64) {
        if bytes > 0 {
            let mut interval = self.interval.lock().expect("UI profile lock");
            interval.upload_calls[kind as usize] += 1;
            interval.upload_bytes[kind as usize] += bytes;
        }
    }

    /// Exposes submitted upload counters to preparation regressions without resetting them.
    #[cfg(test)]
    pub(super) fn submitted_uploads(&self) -> [[u64; 3]; 2] {
        let interval = self.interval.lock().expect("UI profile lock");
        [interval.upload_calls, interval.upload_bytes]
    }

    /// Advances one submitted frame and resets only counters whose interval has elapsed.
    fn finish_frame(&mut self, now: Instant) -> Option<(Duration, Interval)> {
        let interval = self.interval.get_mut().expect("UI profile lock");
        interval.frames += 1;
        let elapsed = now.saturating_duration_since(self.started);
        if elapsed < Duration::from_secs(1) {
            return None;
        }
        self.started = now;
        Some((elapsed, std::mem::take(interval)))
    }
}

/// Registers no resource or reporting system when UI diagnostics are disabled.
pub(crate) fn install(app: &mut SubApp) {
    if crate::gpu_timing::ui_profiling_requested() && !app.world().contains_resource::<UiProfile>()
    {
        app.init_resource::<UiProfile>().add_systems(
            Render,
            flush.in_set(RenderSystems::Render).after(render_system),
        );
    }
}

/// Reports complete submitted frames without placing logging inside the draw loop.
fn flush(mut profile: ResMut<UiProfile>, gpu: Option<Res<super::UiGpu>>) {
    if let Some((elapsed, interval)) = profile.finish_frame(Instant::now()) {
        let mut report = interval.json(elapsed);
        report["baseline_replay"] = profile.baseline_replay.into();
        if let Some(gpu) = gpu
            && let Some(input) = gpu.last_admitted_publication.upgrade()
            && Some(input.revision) == gpu.accepted_revision
        {
            report["publication"] = publication_counts(&input);
        }
        bevy::log::info!("RUST_MCBE_GPU_UI {}", report);
    }
}

/// Describes retained geometry once per report; counts include indices that reuse a vertex.
fn publication_counts(input: &render_model::UiRenderInput) -> serde_json::Value {
    let model_indices: u32 = input
        .batches
        .iter()
        .filter(|batch| batch.isolated_depth_scope.is_some())
        .map(|batch| batch.index_count)
        .sum();
    let sdf_indices = input
        .indices
        .iter()
        .filter(|&&index| {
            input.vertices[index as usize].style_flags & u32::from(assets::FONT_STYLE_SDF) != 0
        })
        .count();
    serde_json::json!({
        "vertices": input.vertices.len(),
        "indices": input.indices.len(),
        "batches": input.batches.len(),
        "model_indices": model_indices,
        "sdf_indices": sdf_indices,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redraw_reuse_and_uploads_count_only_submitted_work() {
        let mut profile = UiProfile::default();
        profile.record_layer(true, [1920, 1080], wgpu::TextureFormat::Rgba8Unorm, 1);
        profile.record_pass(RuntimeStage::GpuUiRaster);
        profile.record_draw(RuntimeStage::GpuUiRaster, 12);
        profile.record_draw(RuntimeStage::GpuUiRaster, 6);
        profile.record_upload(UploadKind::Geometry, 64);
        profile.record_upload(UploadKind::Texture, 256);
        profile.record_upload(UploadKind::Viewport, 16);
        profile.record_upload(UploadKind::Geometry, 0);
        assert!(profile.finish_frame(profile.started).is_none());
        profile.record_layer(false, [1920, 1080], wgpu::TextureFormat::Rgba8Unorm, 1);
        profile.record_pass(RuntimeStage::GpuUiComposite);
        profile.record_draw(RuntimeStage::GpuUiComposite, 0);
        let (_, interval) = profile
            .finish_frame(profile.started + Duration::from_secs(1))
            .unwrap();
        assert_eq!(
            (interval.frames, interval.redraws, interval.reuses),
            (2, 1, 1)
        );
        assert_eq!(interval.passes, [1, 0, 1, 0]);
        assert_eq!(interval.draws, [2, 0, 1, 0]);
        assert_eq!(interval.indices, [18, 0, 0, 0]);
        assert_eq!(interval.upload_calls, [1, 1, 1]);
        assert_eq!(interval.upload_bytes, [64, 256, 16]);
        assert_eq!(*profile.interval.lock().unwrap(), Interval::default());
    }
}
