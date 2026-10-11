//! GPU markers bracket the command buffers of one render system in submission order.

use super::{GpuTimestamps, NO_SPAN, mark};
use crate::RuntimeStage;
use bevy::{
    ecs::{
        schedule::ScheduleConfigs,
        system::{CombinatorSystem, Combine, IntoSystem, RunSystemError, ScheduleSystem, System},
    },
    prelude::{IntoScheduleConfigs, World},
    render::renderer::RenderContext,
};
use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

/// Brackets a render system while preserving its conditions, access and function sets.
pub(crate) fn profiled<S, M>(
    system: S,
    stage: Option<RuntimeStage>,
    name: &'static str,
) -> ScheduleConfigs<ScheduleSystem>
where
    S: IntoSystem<(), (), M>,
{
    let marker = Arc::new(AtomicU32::new(NO_SPAN));
    let begin_marker = marker.clone();
    #[cfg(feature = "tracy")]
    let node_marker = Arc::new(AtomicU32::new(NO_SPAN));
    #[cfg(feature = "tracy")]
    let begin_node_marker = node_marker.clone();
    #[cfg(not(feature = "tracy"))]
    let _ = name;
    let begin = move |world: &World, mut context: RenderContext| {
        begin_marker.store(NO_SPAN, Ordering::Relaxed);
        if let Some(stage) = stage
            && let Some((span, close)) = world
                .get_resource::<GpuTimestamps>()
                .and_then(|timestamps| timestamps.open_graph_span(stage))
        {
            mark(context.command_encoder(), span.queries, span.begin);
            if close {
                begin_marker.store(span.begin, Ordering::Relaxed);
            }
        }
        #[cfg(feature = "tracy")]
        {
            begin_node_marker.store(NO_SPAN, Ordering::Relaxed);
            if let Some(span) = super::nodes::open_node(world, name) {
                span.begin(context.command_encoder());
                begin_node_marker.store(span.begin, Ordering::Relaxed);
            }
        }
    };
    let end = move |world: &World, mut context: RenderContext| {
        let begin = marker.swap(NO_SPAN, Ordering::Relaxed);
        if begin != NO_SPAN
            && let Some(timestamps) = world.get_resource::<GpuTimestamps>()
        {
            mark(context.command_encoder(), &timestamps.queries, begin + 1);
        }
        #[cfg(feature = "tracy")]
        super::nodes::close_node(
            world,
            &mut context,
            node_marker.swap(NO_SPAN, Ordering::Relaxed),
        );
    };
    let system = IntoSystem::into_system(system);
    let name = system.name();
    let finish =
        CombinatorSystem::<AlwaysFinish, _, _>::new(system, IntoSystem::into_system(end), name);
    IntoSystem::into_system(begin).pipe(finish).into_configs()
}

/// Closes a claimed span even when a view query skips its draw system.
struct AlwaysFinish;

impl<A: System<In = (), Out = ()>, B: System<In = (), Out = ()>> Combine<A, B> for AlwaysFinish {
    type In = ();
    type Out = ();

    fn combine<T>(
        _: (),
        data: &mut T,
        run: impl FnOnce((), &mut T) -> Result<(), RunSystemError>,
        finish: impl FnOnce((), &mut T) -> Result<(), RunSystemError>,
    ) -> Result<(), RunSystemError> {
        let result = run((), data);
        finish((), data)?;
        result
    }
}
