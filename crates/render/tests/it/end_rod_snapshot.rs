//! Offline end-rod coverage with an explicitly supplied original-art carrier.
use {
    crate::gpu_snapshot,
    crate::gpu_snapshot::Gpu,
    crate::rod_snapshot::{fixture, mesh, render},
};

/// Returns the mounting neighbour offset for the active facing state.
fn support_offset(direction: usize) -> [i32; 3] {
    [
        [0, 1, 0],
        [0, -1, 0],
        [0, 0, -1],
        [0, 0, 1],
        [-1, 0, 0],
        [1, 0, 0],
    ][direction]
}

#[test]
fn end_rods_render_complete_shafts_with_support_for_every_facing() {
    let Some((assets, records)) = fixture("CINNABAR_END_ROD_CARRIER") else {
        return;
    };
    let Some(gpu) = Gpu::for_fixture("end-rod terrain snapshot") else {
        return;
    };
    eprintln!("end-rod GPU backend: {:?}", gpu.backend);
    let id = |name: &str| {
        records
            .iter()
            .find(|r| r.name.as_ref() == name)
            .unwrap()
            .sequential_id
    };
    let mut complete = true;
    let mut directions = Vec::new();
    for record in records
        .iter()
        .filter(|r| r.name.as_ref() == "minecraft:end_rod")
    {
        let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        let direction = state["facing_direction"]["value"].as_u64().unwrap() as usize;
        directions.push(direction);
        let mesh = mesh(
            &assets,
            id("minecraft:air"),
            id("minecraft:stone"),
            record.sequential_id,
            support_offset(direction),
        );
        let pixels = render(&gpu, &assets, &mesh, support_offset(direction), true);
        let support_pixels = render(&gpu, &assets, &mesh, support_offset(direction), false);
        gpu_snapshot::save(&format!("end-rod-facing-{direction}"), &pixels);
        let drawn = pixels
            .as_chunks::<4>()
            .0
            .iter()
            .zip(support_pixels.as_chunks::<4>().0)
            .filter(|(pixel, support)| pixel != support)
            .count();
        eprintln!(
            "facing {direction}: {} opaque and {} blended model quads, {drawn} visible pixels",
            mesh.model_draw_refs().len(),
            mesh.transparent_model_draw_refs().len()
        );
        complete &= mesh.model_draw_refs().len() == 12 && drawn > 400;
    }
    directions.sort_unstable();
    assert_eq!(directions, [0, 1, 2, 3, 4, 5]);
    assert!(
        complete,
        "all rods retain their complete shaft, sprite coverage and base beside support"
    );
}
