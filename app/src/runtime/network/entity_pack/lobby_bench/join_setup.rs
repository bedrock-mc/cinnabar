//! Repeatable cold actor publication against an external captured lobby and pack.
use super::*;

/// Covers all registered vertex data, including geometry not yet used by a drawn actor.
fn geometry_digest(frame: &ActorRenderFrame) -> u64 {
    use std::hash::Hasher;
    let mut digest = std::collections::hash_map::DefaultHasher::new();
    for span in frame.rig.geometry_spans.iter() {
        digest.write_u32(span.first_vertex);
        digest.write_u32(span.vertex_count);
        for vertex in frame.rig.geometry_vertices.span(*span).unwrap() {
            for value in vertex
                .position
                .into_iter()
                .chain(vertex.normal)
                .chain(vertex.uv)
                .chain(vertex.back_uv)
            {
                digest.write_u32(value.to_bits());
            }
            digest.write_u32(vertex.bone_index);
        }
    }
    digest.finish()
}

/// Includes the first session's geometry/artwork setup, which the warm replay discards.
#[test]
#[ignore = "requires a captured server session, its pack and local carriers"]
fn lobby_join_setup_bench() {
    let capture = std::env::var_os("CINNABAR_LOBBY_CAPTURE").expect("captured lobby required");
    let pack = std::env::var_os("CINNABAR_RENDER_PACK").expect("captured pack required");
    let capture = read_capture(Path::new(&capture));
    for trial in 0..3 {
        let (mut world, _, _) = build_world(&capture, Path::new(&pack), false);
        world
            .resource_mut::<Time<Real>>()
            .update_with_instant(Instant::now());
        let allocated = crate::tests::alloc_count::thread_allocations();
        let started = Instant::now();
        let cpu_started = thread_cpu_time();
        world.run_system_cached(prepare_actor_render_frame).unwrap();
        world.run_system_cached(publish_actor_render_frame).unwrap();
        let elapsed = started.elapsed();
        let cpu = thread_cpu_time() - cpu_started;
        let allocations = crate::tests::alloc_count::thread_allocations() - allocated;
        let snapshot = world
            .resource::<RuntimeStageProfiler>()
            .take_snapshot_if_due(Duration::ZERO)
            .unwrap();
        let frame = world.resource::<ActorRenderFrame>();
        eprintln!(
            "LOBBY_JOIN_SETUP {}",
            serde_json::json!({
                "trial": trial, "wall_ms": elapsed.as_secs_f64() * 1e3,
                "cpu_ms": cpu.as_secs_f64() * 1e3, "allocations": allocations,
                "session_setup_ms": snapshot.samples[RuntimeStage::ActorSessionSetup as usize].total.as_secs_f64() * 1e3,
                "geometry_setup_ms": snapshot.samples[RuntimeStage::ActorGeometrySetup as usize].total.as_secs_f64() * 1e3,
                "artwork_setup_ms": snapshot.samples[RuntimeStage::ActorArtworkSetup as usize].total.as_secs_f64() * 1e3,
                "equipment_setup_ms": snapshot.samples[RuntimeStage::ActorEquipmentSetup as usize].total.as_secs_f64() * 1e3,
                "frame_digest": format!("{:016x}", frame_digest(frame)),
                "geometry_digest": format!("{:016x}", geometry_digest(frame)),
                "artwork_identity": frame.artwork_pages().identity(),
                "artwork_bytes": frame.artwork_pages().pages().iter().map(|page| page.shared_pixels().len()).sum::<usize>(),
            })
        );
    }
}
