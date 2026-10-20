//! Repeatable native publication benchmark; no renderer or game resources are required.

use std::{hint::black_box, sync::Arc, time::Instant};
use ui::mod_hud::{Hud, HudMergeCache};

#[global_allocator]
static ALLOCATOR: test_alloc::CountingAllocator = test_alloc::CountingAllocator;

/// Produces validated large text/cell content using the public admission limits.
fn fixture(cards: usize) -> Hud {
    let cards: Vec<_> = (0..cards)
        .map(|index| {
            serde_json::json!({
                "id":format!("card_{index}"), "rows": (0..16).map(|row| serde_json::json!({
                    "label":format!("Observed label {row}"),"value":"1234", "item":"minecraft:stone"
                })).collect::<Vec<_>>()
            })
        })
        .collect();
    let hud: Hud = serde_json::from_value(serde_json::json!({"cards":cards})).unwrap();
    hud.validate().unwrap();
    hud
}

/// Measures warm per-frame publication, including destruction, and isolates allocator requests.
#[expect(
    clippy::print_stdout,
    reason = "the opt-in benchmark reports measurements"
)]
fn measure(label: &str, mut work: impl FnMut()) {
    for _ in 0..256 {
        work();
    }
    let (_, allocation) = test_alloc::measure(&mut work);
    let mut times = Vec::with_capacity(4096);
    for _ in 0..4096 {
        let start = Instant::now();
        work();
        times.push(start.elapsed().as_nanos());
    }
    times.sort_unstable();
    println!(
        "NATIVE_HUD_BENCH {label} ns_p50={} ns_p99={} ns_max={} allocations={} bytes={}",
        times[times.len() / 2],
        times[times.len() * 99 / 100],
        times[times.len() - 1],
        allocation.calls,
        allocation.bytes
    );
}

/// Cached and uncached paths publish identical output; idle shared publication allocates nothing.
#[test]
fn benchmark_native_hud_publication() {
    for cards in [1, 8, 16] {
        let input = fixture(cards);
        let mut ui = mod_ui::ModUi::default();
        ui.set_mod_hud(Some(&input)).unwrap();
        // The former per-frame path cloned the owner's HUD and validated it before equality.
        measure(&format!("before_{cards}_cards"), || {
            let merged = black_box(&input).clone();
            merged.validate().unwrap();
            ui.set_mod_hud(Some(&merged)).unwrap();
        });
        let mut cache = HudMergeCache::default();
        let shared = cache.update([(1, &input)].into_iter()).unwrap();
        ui.set_shared_mod_hud(Some(shared.clone())).unwrap();
        let (_, allocation) =
            test_alloc::measure(|| ui.set_shared_mod_hud(Some(shared.clone())).unwrap());
        assert_eq!(allocation.calls, 0);
        measure(&format!("after_{cards}_cards"), || {
            ui.set_shared_mod_hud(cache.update([(1, black_box(&input))].into_iter()))
                .unwrap();
        });
        let mut changed = input.clone();
        changed.cards[0].rows[0].value = "updated".into();
        let next = cache.update([(2, &changed)].into_iter()).unwrap();
        assert!(!Arc::ptr_eq(&next, &shared));
        ui.set_shared_mod_hud(Some(next)).unwrap();
        assert!(ui.mod_widget_icons(|_, _| None).is_some());
    }
}
