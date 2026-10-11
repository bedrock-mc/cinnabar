//! Offline lightning-rod coverage with an explicitly supplied original-art carrier.
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
        [0, 0, 1],
        [0, 0, -1],
        [1, 0, 0],
        [-1, 0, 0],
    ][direction]
}

#[test]
fn lightning_rods_render_complete_shafts_with_support_for_every_facing() {
    let Some((assets, records)) = fixture("CINNABAR_LIGHTNING_ROD_CARRIER") else {
        return;
    };
    let Some(gpu) = Gpu::for_fixture("lightning-rod terrain snapshot") else {
        return;
    };
    eprintln!("lightning-rod GPU backend: {:?}", gpu.backend);
    let id = |name: &str| {
        records
            .iter()
            .find(|r| r.name.as_ref() == name)
            .unwrap()
            .sequential_id
    };
    let mut complete = true;
    let mut states = 0;
    for record in records.iter().filter(|r| r.name.ends_with("lightning_rod")) {
        let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        let direction = state["facing_direction"]["value"].as_u64().unwrap() as usize;
        states += 1;
        let powered = state["powered_bit"]["value"].as_u64().unwrap();
        let mesh = mesh(
            &assets,
            id("minecraft:air"),
            id("minecraft:stone"),
            record.sequential_id,
            support_offset(direction),
        );
        let pixels = render(&gpu, &assets, &mesh, support_offset(direction), true);
        let support_pixels = render(&gpu, &assets, &mesh, support_offset(direction), false);
        gpu_snapshot::save(
            &format!(
                "{}-facing-{direction}-powered-{powered}",
                record.name.strip_prefix("minecraft:").unwrap()
            ),
            &pixels,
        );
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
        complete &=
            mesh.model_draw_refs().len() == if direction < 2 { 11 } else { 12 } && drawn > 400;
    }
    assert_eq!(states, 96);
    assert!(
        complete,
        "all rods retain their complete shaft, sprite coverage and tip beside support"
    );
}
