//! Main-schedule phase spans: time no named stage covers is still attributed to the phase that
//! spent it, so a slow frame never reports main-thread work under no stage at all.

use bevy::app::{App, Last, MainScheduleOrder, PreUpdate, RunFixedMainLoop, Update};
use bevy::ecs::schedule::{IntoScheduleConfigs, ScheduleLabel};

use crate::runtime_profile::{RuntimeStage, RuntimeStageSpans, begin_stage_span, end_stage_span};

const PRE_UPDATE: usize = RuntimeStage::MainPreUpdate as usize;
const FIXED_UPDATE: usize = RuntimeStage::MainFixedUpdate as usize;
const UPDATE: usize = RuntimeStage::MainUpdate as usize;
const POST_UPDATE: usize = RuntimeStage::MainPostUpdate as usize;

/// One-system schedules run between two main-schedule phases.
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
enum PhaseBoundary {
    BeforePreUpdate,
    AfterPreUpdate,
    AfterFixedUpdate,
    AfterUpdate,
    BeforeLast,
}

/// Times the main schedule phases as RuntimeStage spans, including SpawnScene in PostUpdate.
/// Boundary schedules keep neighboring phase systems outside each measured span.
pub fn install_main_phase_spans(app: &mut App) {
    app.init_resource::<RuntimeStageSpans>();
    let mut order = app.world_mut().resource_mut::<MainScheduleOrder>();
    order.insert_before(PreUpdate, PhaseBoundary::BeforePreUpdate);
    order.insert_after(PreUpdate, PhaseBoundary::AfterPreUpdate);
    order.insert_after(RunFixedMainLoop, PhaseBoundary::AfterFixedUpdate);
    order.insert_after(Update, PhaseBoundary::AfterUpdate);
    order.insert_before(Last, PhaseBoundary::BeforeLast);
    app.add_systems(
        PhaseBoundary::BeforePreUpdate,
        begin_stage_span::<PRE_UPDATE>,
    )
    .add_systems(
        PhaseBoundary::AfterPreUpdate,
        (
            end_stage_span::<PRE_UPDATE>,
            begin_stage_span::<FIXED_UPDATE>,
        )
            .chain(),
    )
    .add_systems(
        PhaseBoundary::AfterFixedUpdate,
        (end_stage_span::<FIXED_UPDATE>, begin_stage_span::<UPDATE>).chain(),
    )
    .add_systems(
        PhaseBoundary::AfterUpdate,
        (end_stage_span::<UPDATE>, begin_stage_span::<POST_UPDATE>).chain(),
    )
    .add_systems(PhaseBoundary::BeforeLast, end_stage_span::<POST_UPDATE>);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RuntimeStageProfiler;
    use bevy::prelude::{App, PostUpdate, PreUpdate, Res, Update};
    use std::time::Duration;

    /// Records a zero-length probe span from inside a phase.
    fn probe<const S: usize>(profiler: Res<RuntimeStageProfiler>) {
        drop(profiler.time(RuntimeStage::ALL[S]));
    }

    /// `(start, end)` of every traced span named `name`, in microseconds.
    fn spans(trace: &serde_json::Value, name: &str) -> Vec<(f64, f64)> {
        trace["traceEvents"]
            .as_array()
            .expect("trace events")
            .iter()
            .filter(|event| event["name"] == name)
            .map(|event| {
                let start = event["ts"].as_f64().expect("start");
                (start, start + event["dur"].as_f64().expect("duration"))
            })
            .collect()
    }

    /// Work a system does in a phase falls inside that phase's span and no other's.
    #[test]
    fn each_phase_span_encloses_only_its_own_systems() {
        const IN_PRE_UPDATE: usize = RuntimeStage::Particles as usize;
        const IN_UPDATE: usize = RuntimeStage::Audio as usize;
        const IN_POST_UPDATE: usize = RuntimeStage::BlockEntities as usize;
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("trace.json");
        let profiler = RuntimeStageProfiler::with_trace(true, Some(path.clone()));
        let mut app = App::new();
        app.insert_resource(profiler.clone())
            .add_systems(PreUpdate, probe::<IN_PRE_UPDATE>)
            .add_systems(Update, probe::<IN_UPDATE>)
            .add_systems(PostUpdate, probe::<IN_POST_UPDATE>);
        install_main_phase_spans(&mut app);
        app.update();
        app.update();
        let snapshot = profiler.take_snapshot_if_due(Duration::ZERO).unwrap();
        for stage in [
            RuntimeStage::MainPreUpdate,
            RuntimeStage::MainFixedUpdate,
            RuntimeStage::MainUpdate,
            RuntimeStage::MainPostUpdate,
        ] {
            assert_eq!(
                snapshot.samples[stage as usize].count,
                2,
                "{}",
                stage.name()
            );
        }
        profiler.flush_trace();
        let trace: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let phases = [
            ("main_pre_update", RuntimeStage::ALL[IN_PRE_UPDATE].name()),
            ("main_fixed_update", ""),
            ("main_update", RuntimeStage::ALL[IN_UPDATE].name()),
            ("main_post_update", RuntimeStage::ALL[IN_POST_UPDATE].name()),
        ];
        let probes = phases.iter().filter(|(_, probe)| !probe.is_empty());
        for (_, probe) in probes {
            for (start, end) in spans(&trace, probe) {
                let enclosing: Vec<_> = phases
                    .iter()
                    .filter(|(phase, _)| {
                        spans(&trace, phase)
                            .iter()
                            .any(|&(low, high)| low <= start && end <= high)
                    })
                    .map(|(phase, _)| *phase)
                    .collect();
                let owner = phases
                    .iter()
                    .find(|(_, owned)| owned == probe)
                    .map(|(phase, _)| *phase);
                assert_eq!(enclosing, Vec::from_iter(owner), "{probe}");
            }
        }
    }
}
