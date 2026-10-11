use {super::*, launcher::menu::MenuScreen};

/// Captures comparable CPU build work without using timing as a CI assertion.
#[test]
fn oreui_work_probe() {
    if std::env::var_os("CINNABAR_OREUI_WORK_PROBE").is_none() {
        return;
    }
    let Some(mut bench) = Bench::new() else {
        eprintln!("skipping oreui_work_probe: missing local UI carrier (make assets)");
        return;
    };
    bench.runtime.begin_session(0);
    grow(&mut bench.view);
    for (name, screen) in [
        ("home", MenuScreen::Home),
        ("servers", MenuScreen::Servers),
        ("settings", MenuScreen::Settings),
    ] {
        bench.view.screen = screen;
        for _ in 0..60 {
            bench.frame();
        }
        for scenario in ["idle", "hover", "scroll", "open"] {
            let actions = bench.hits();
            let mut samples = Vec::with_capacity(400);
            let mut allocations = 0;
            let before = bench.presentation.tree_builds;
            let paints = bench.presentation.oreui_paints;
            for frame in 0..400 {
                match scenario {
                    "hover" => {
                        bench.view.hovered = actions.get((frame / 12) % actions.len()).copied()
                    }
                    "scroll" => {
                        if frame % 12 == 0 {
                            bench.presentation.scroll_menu(
                                ui::UiPoint::new(
                                    if screen == MenuScreen::Settings {
                                        1000.0
                                    } else {
                                        100.0
                                    },
                                    500.0,
                                )
                                .unwrap(),
                                if frame % 24 == 0 { -2.0 } else { 2.0 },
                                false,
                            );
                        }
                    }
                    "open" => {
                        bench.view.screen = if frame % 24 < 12 {
                            if screen == MenuScreen::Home {
                                MenuScreen::Settings
                            } else {
                                MenuScreen::Home
                            }
                        } else {
                            screen
                        };
                    }
                    _ => {}
                }
                let (elapsed, count) = crate::allocation_count::count(|| bench.frame());
                samples.push(elapsed);
                allocations += count;
            }
            samples.sort();
            eprintln!(
                "oreui-work {name} {scenario} p50_ms={:.4} p99_ms={:.4} max_ms={:.4} allocations_per_frame={:.1} tree_builds={} oreui_paints={}",
                samples[199].as_secs_f64() * 1000.0,
                samples[395].as_secs_f64() * 1000.0,
                samples[399].as_secs_f64() * 1000.0,
                allocations as f64 / 400.0,
                bench.presentation.tree_builds - before,
                bench.presentation.oreui_paints - paints
            );
            bench.view.screen = screen;
            bench.view.hovered = None;
            for _ in 0..60 {
                bench.frame();
            }
        }
    }
}
