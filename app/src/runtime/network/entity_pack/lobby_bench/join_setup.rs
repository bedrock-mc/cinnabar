//! Repeatable actor publication against an offline packet fixture and pack.
use super::*;

/// Covers all registered vertex data, including geometry not yet used by a drawn actor.
fn geometry_digest(frame: &ActorRenderFrame) -> u64 {
    use std::hash::Hasher;
    let mut digest = std::collections::hash_map::DefaultHasher::new();
    for span in frame.rig.geometry_spans.iter() {
        digest.write_u32(span.first_vertex);
        digest.write_u32(span.vertex_count);
        hash_vertices(
            &mut digest,
            frame.rig.geometry_vertices.span(*span).unwrap(),
        );
    }
    digest.finish()
}

/// Hashes the exact vertex payload without depending on its storage address.
fn hash_vertices(digest: &mut impl std::hash::Hasher, vertices: &[render::ActorRigVertex]) {
    for vertex in vertices {
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

/// Covers the multiset of full geometry payloads independently of catalog placement.
fn geometry_payload_digest(frame: &ActorRenderFrame) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut payloads = frame
        .rig
        .geometry_spans
        .iter()
        .map(|span| {
            let mut digest = std::collections::hash_map::DefaultHasher::new();
            hash_vertices(
                &mut digest,
                frame.rig.geometry_vertices.span(*span).unwrap(),
            );
            (span.vertex_count, digest.finish())
        })
        .collect::<Vec<_>>();
    payloads.sort_unstable();
    let mut digest = std::collections::hash_map::DefaultHasher::new();
    payloads.hash(&mut digest);
    digest.finish()
}

/// Includes initial setup and the item refresh that follows the same accepted entity pack.
#[test]
#[ignore = "requires an offline packet fixture, its pack and local carriers"]
fn lobby_join_setup_bench() {
    let capture = std::env::var_os("CINNABAR_LOBBY_CAPTURE").expect("captured lobby required");
    let pack = std::env::var_os("CINNABAR_RENDER_PACK").expect("captured pack required");
    let capture = read_capture(Path::new(&capture));
    for trial in 0..3 {
        let (mut world, _, _) = build_world(&capture, Path::new(&pack), false);
        world
            .resource_mut::<Time<Real>>()
            .update_with_instant(Instant::now());
        if std::env::var_os("CINNABAR_PREPARE_ACTOR_ARTWORK").is_some() {
            prepare_artwork(&mut world, trial);
        }
        measure_setup(&mut world, trial, "initial");
        world
            .resource_mut::<crate::runtime::world::ClientWorld>()
            .session_items = Some(Arc::new(super::super::SessionItems {
            components: Arc::default(),
            icons: None,
        }));
        measure_setup(&mut world, trial, "items_refresh");
    }
}

/// Reports worker preparation separately so moving work cannot be mistaken for removing it.
fn prepare_artwork(world: &mut World, trial: usize) {
    use crate::runtime::network::prepared_actor_artwork::PreparedActorArtwork;
    let base = world.resource::<render::ActorArtworkPages>().clone();
    let pack = world
        .resource::<crate::runtime::world::ClientWorld>()
        .pack_entities
        .clone()
        .expect("benchmark session pack");
    let prepared = std::thread::spawn(move || {
        let allocated = crate::tests::alloc_count::thread_allocations();
        let started = Instant::now();
        let cpu_started = thread_cpu_time();
        let prepared = PreparedActorArtwork::new(&base, &pack);
        let wall = started.elapsed();
        let cpu = thread_cpu_time()
            .zip(cpu_started)
            .map(|(after, before)| (after - before).as_secs_f64() * 1e3);
        let allocations = crate::tests::alloc_count::thread_allocations() - allocated;
        eprintln!(
            "LOBBY_JOIN_ARTWORK_PREP {}",
            serde_json::json!({
                "trial": trial,
                "wall_ms": wall.as_secs_f64() * 1e3,
                "cpu_ms": cpu,
                "allocations": allocations,
            })
        );
        Arc::new(prepared)
    })
    .join()
    .unwrap();
    world
        .resource_mut::<crate::runtime::world::ClientWorld>()
        .prepared_actor_artwork = Some(prepared);
}

/// Measures only publication; hashes outside the timed region cover every geometry and artwork page.
fn measure_setup(world: &mut World, trial: usize, phase: &str) {
    let allocated = crate::tests::alloc_count::thread_allocations();
    let started = Instant::now();
    let cpu_started = thread_cpu_time();
    world.run_system_cached(prepare_actor_render_frame).unwrap();
    world.run_system_cached(publish_actor_render_frame).unwrap();
    let elapsed = started.elapsed();
    let cpu = thread_cpu_time()
        .zip(cpu_started)
        .map(|(after, before)| (after - before).as_secs_f64() * 1e3);
    let allocations = crate::tests::alloc_count::thread_allocations() - allocated;
    let snapshot = world
        .resource::<RuntimeStageProfiler>()
        .take_snapshot_if_due(Duration::ZERO)
        .unwrap();
    let frame = world.resource::<ActorRenderFrame>();
    eprintln!(
        "LOBBY_JOIN_SETUP {}",
        serde_json::json!({
            "trial": trial, "phase": phase, "wall_ms": elapsed.as_secs_f64() * 1e3,
            "cpu_ms": cpu, "allocations": allocations,
            "session_setup_ms": snapshot.samples[RuntimeStage::ActorSessionSetup as usize].total.as_secs_f64() * 1e3,
            "geometry_setup_ms": snapshot.samples[RuntimeStage::ActorGeometrySetup as usize].total.as_secs_f64() * 1e3,
            "artwork_setup_ms": snapshot.samples[RuntimeStage::ActorArtworkSetup as usize].total.as_secs_f64() * 1e3,
            "equipment_setup_ms": snapshot.samples[RuntimeStage::ActorEquipmentSetup as usize].total.as_secs_f64() * 1e3,
            "frame_digest": format!("{:016x}", frame_digest(frame)),
            "geometry_digest": format!("{:016x}", geometry_digest(frame)),
            "geometry_payload_digest": format!("{:016x}", geometry_payload_digest(frame)),
            "artwork_identity": frame.artwork_pages().identity(),
            "artwork_bytes": frame.artwork_pages().pages().iter().map(|page| page.shared_pixels().len()).sum::<usize>(),
        })
    );
}
