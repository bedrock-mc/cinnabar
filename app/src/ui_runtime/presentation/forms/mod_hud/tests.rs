use super::super::{ServerUiPack, pack_harness, snapshot, tests::mini_engine_presentation};
use super::*;
use render::UiRenderInput;
use ui::DpiScale;

const SAMPLE_COMPONENT_ENV: &str = "CINNABAR_MOD_SNAPSHOT_COMPONENT";

/// A real engine over a small HUD fixture, independent of local carrier files.
fn presentation() -> UiPresentationRuntime {
    let mut presentation = mini_engine_presentation();
    presentation.set_server_ui_pack(&hud_pack(
        br#"{
        "namespace": "hud",
        "hud_screen": { "type": "screen", "controls": [{ "label": {
            "type": "label", "size": [100, 12],
            "anchor_from": "bottom_middle", "anchor_to": "bottom_middle",
            "text": "Base HUD" } }] }
    }"#,
    ));
    presentation
}

/// Registers the HUD fixture as a loadable pack document.
fn hud_pack(hud: &[u8]) -> ServerUiPack {
    ServerUiPack {
        ui_layers: vec![vec![
            (
                "ui/_ui_defs.json".to_owned(),
                br#"{"ui_defs":["ui/hud_screen.json"]}"#.to_vec(),
            ),
            ("ui/hud_screen.json".to_owned(), hud.to_vec()),
        ]],
        ..Default::default()
    }
}

/// Builds the same offline frame so only the extension state can change its pixels.
fn frame(presentation: &mut UiPresentationRuntime) -> UiRenderInput {
    presentation
        .build(
            &UiRuntime::new(1),
            0,
            [1280, 720],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap()
}

#[test]
fn no_mod_output_is_identical_to_the_vanilla_frame() {
    let mut presentation = presentation();
    let before = frame(&mut presentation);
    presentation.set_mod_label(None).unwrap();
    let after = frame(&mut presentation);
    assert_eq!(before, after);
    assert!(presentation.form_presentation.mod_hud.is_none());
}

#[test]
fn extension_mount_update_and_revoke_use_json_ui() {
    let mut presentation = presentation();
    let before = snapshot::rasterize(&frame(&mut presentation));
    presentation
        .set_mod_label(Some("Cinnabar extension: Hello"))
        .unwrap();
    let first = frame(&mut presentation);
    assert!(presentation.form_presentation.hud.hud.has_visible_content());
    assert!(
        presentation
            .form_presentation
            .mod_hud
            .as_ref()
            .unwrap()
            .screen
            .passes
            > 0
    );
    assert!(before != snapshot::rasterize(&first));
    assert_eq!(first, frame(&mut presentation));
    assert_eq!(
        presentation
            .form_presentation
            .mod_hud
            .as_ref()
            .unwrap()
            .screen
            .passes,
        1
    );
    presentation
        .set_mod_label(Some("Cinnabar extension: F8 pressed"))
        .unwrap();
    let updated = frame(&mut presentation);
    assert_ne!(first.vertices, updated.vertices);
    presentation.set_mod_label(None).unwrap();
    assert_eq!(before, snapshot::rasterize(&frame(&mut presentation)));
}

#[test]
fn extension_is_hidden_while_chat_or_inventory_owns_input() {
    for inventory in [false, true] {
        let mut presentation = presentation();
        let mut runtime = UiRuntime::new(1);
        runtime.inventory_open = inventory;
        runtime.chat_focused = !inventory;
        let build = |presentation: &mut UiPresentationRuntime| {
            presentation
                .build(&runtime, 0, [1280, 720], DpiScale::new(1.0).unwrap())
                .unwrap()
        };
        let before = snapshot::rasterize(&build(&mut presentation));
        presentation
            .set_mod_label(Some("Hidden extension"))
            .unwrap();
        assert_eq!(before, snapshot::rasterize(&build(&mut presentation)));
        assert_eq!(
            presentation
                .form_presentation
                .mod_hud
                .as_ref()
                .unwrap()
                .screen
                .passes,
            0
        );
    }
}

#[test]
fn extension_cannot_restore_a_server_hidden_hud() {
    let mut presentation = presentation();
    presentation.set_server_ui_pack(&hud_pack(
        br#"{
        "namespace": "hud", "hud_screen": { "type": "screen", "visible": false }
    }"#,
    ));
    let before = frame(&mut presentation);
    presentation
        .set_mod_label(Some("Hidden extension"))
        .unwrap();
    assert_eq!(before, frame(&mut presentation));
    assert_eq!(
        presentation
            .form_presentation
            .mod_hud
            .as_ref()
            .unwrap()
            .screen
            .passes,
        0
    );
}

#[test]
fn mod_spike_snapshot_with_real_carrier() {
    let Some(mut presentation) = pack_harness::engine_presentation() else {
        assert!(
            std::env::var_os("CINNABAR_FORM_SNAPSHOT_DIR").is_none(),
            "snapshot requested without UI carrier"
        );
        eprintln!(
            "skipping mod_spike_snapshot_with_real_carrier: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let before = frame(&mut presentation);
    snapshot::write(&before, "mod-spike-before");
    let mut guest = std::env::var_os(SAMPLE_COMPONENT_ENV).map(|path| {
        mod_host::ModHost::load_with_grants(
            std::path::Path::new(&path),
            mod_host::ModGrants { environment: true },
        )
        .unwrap()
    });
    let text = guest
        .as_ref()
        .map(|host| host.label().expect("sample must publish a label"))
        .unwrap_or("Cinnabar extension: Hello (Press demo key)");
    presentation.set_mod_label(Some(text)).unwrap();
    let after = frame(&mut presentation);
    snapshot::write(&after, "mod-spike-after");
    assert_ne!(snapshot::rasterize(&before), snapshot::rasterize(&after));
    if let Some(host) = guest.as_mut() {
        host.frame(true).unwrap();
        presentation.set_mod_label(host.label()).unwrap();
        let pressed = frame(&mut presentation);
        snapshot::write(&pressed, "mod-spike-keybind");
        assert_ne!(snapshot::rasterize(&after), snapshot::rasterize(&pressed));
    }
    presentation.set_mod_label(None).unwrap();
    assert_eq!(
        snapshot::rasterize(&before),
        snapshot::rasterize(&frame(&mut presentation))
    );
}

#[test]
#[ignore = "requires the real carrier and a compiled sample; prints offline CPU timings"]
fn mod_spike_offline_frame_overhead() {
    let path = std::env::var_os(SAMPLE_COMPONENT_ENV)
        .unwrap_or_else(|| panic!("set {SAMPLE_COMPONENT_ENV} to the compiled sample"));
    let mut host = mod_host::ModHost::load_with_grants(
        std::path::Path::new(&path),
        mod_host::ModGrants { environment: true },
    )
    .unwrap();
    let mut vanilla = pack_harness::engine_presentation().expect("real UI carrier required");
    let mut modded = pack_harness::engine_presentation().expect("real UI carrier required");
    for _ in 0..5 {
        measure_frame_batch(&mut vanilla, None);
        measure_frame_batch(&mut modded, Some(&mut host));
    }
    let mut empty = Vec::new();
    let mut loaded = Vec::new();
    let mut delta = Vec::new();
    for batch in 0..40 {
        let (a, b) = if batch % 2 == 0 {
            let a = measure_frame_batch(&mut vanilla, None);
            (a, measure_frame_batch(&mut modded, Some(&mut host)))
        } else {
            let b = measure_frame_batch(&mut modded, Some(&mut host));
            (measure_frame_batch(&mut vanilla, None), b)
        };
        empty.push(a);
        loaded.push(b);
        delta.push(b - a);
    }
    println!(
        "offline_ui profile={} arch={} os={} viewport=1280x720 dpi=1 batches=40 frames_per_batch={BENCH_FRAMES}",
        if cfg!(debug_assertions) {
            "dev"
        } else {
            "release"
        },
        std::env::consts::ARCH,
        std::env::consts::OS,
    );
    for (name, mut samples) in [
        ("zero_mods", empty),
        ("sample_idle", loaded),
        ("added", delta),
    ] {
        samples.sort_by(f64::total_cmp);
        println!(
            "{name} frame_ns_p50={:.1} frame_ns_p95={:.1}",
            samples[samples.len() / 2],
            samples[samples.len() * 95 / 100],
        );
    }
}

const BENCH_FRAMES: usize = 100;

/// Times the real CPU UI build plus the loaded guest and retained-label adapter.
fn measure_frame_batch(
    presentation: &mut UiPresentationRuntime,
    mut host: Option<&mut mod_host::ModHost>,
) -> f64 {
    let start = std::time::Instant::now();
    for _ in 0..BENCH_FRAMES {
        if let Some(host) = host.as_deref_mut() {
            host.frame(std::hint::black_box(false)).unwrap();
            presentation.set_mod_label(host.label()).unwrap();
        }
        std::hint::black_box(frame(presentation));
    }
    start.elapsed().as_nanos() as f64 / BENCH_FRAMES as f64
}
