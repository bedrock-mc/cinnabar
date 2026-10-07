//! Repeatable HUD bind/layout costs against the owner's local vanilla templates.

#[path = "it/support/frame_stats.rs"]
mod frame_stats;
#[path = "it/support/mod.rs"]
mod support;

#[global_allocator]
static ALLOCATOR: frame_stats::CountedAllocator = frame_stats::CountedAllocator;

use std::{hint::black_box, path::PathBuf, sync::Arc, time::Instant};

use json_ui::{
    BindState, BossBar, CachedLibrary, Catalog, CatalogLibrary, Context, HUD_SCREEN, HudModel,
    LayoutEnv, ResolveCache, Sidebar, TextMeasure, TextureMeta, TextureSource, Timed, ViewState,
    bind_shared, bind_stateful, hud_context, hud_data_source, render_bound, render_bound_cached,
    resolve,
};

/// Stable font metrics keep this benchmark independent of the rasterizer and GPU.
struct FixedText;

impl TextMeasure for FixedText {
    /// Measure the same nine-pixel lines on every run.
    fn extent(&self, text: &str) -> [f64; 2] {
        [text.chars().count() as f64 * 6.0, 9.0]
    }
}

/// Fixed texture metadata avoids file reads inside the timed loop.
struct FixedTextures;

impl TextureSource for FixedTextures {
    /// Supply a stable size for every texture referenced by the templates.
    fn texture(&self, _path: &str) -> Option<TextureMeta> {
        Some(TextureMeta {
            base_size: [16.0; 2],
            pixels: [16.0; 2],
            nineslice: None,
        })
    }
}

/// Every `ui/*.json` file under `root`, keyed by its pack-relative path.
fn pack_files(root: &std::path::Path) -> std::io::Result<Vec<(String, Vec<u8>)>> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).map_err(|error| {
            std::io::Error::new(error.kind(), format!("{}: {error}", dir.display()))
        })? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                stack.push(path);
            } else {
                let relative = path
                    .strip_prefix(root)
                    .expect("entry belongs to its root")
                    .to_string_lossy()
                    .replace('\\', "/");
                if relative.starts_with("ui/") && relative.ends_with(".json") {
                    let bytes = std::fs::read(&path).map_err(|error| {
                        std::io::Error::new(error.kind(), format!("{}: {error}", path.display()))
                    })?;
                    out.push((relative, bytes));
                }
            }
        }
    }
    Ok(out)
}

#[test]
#[ignore = "benchmark; needs the local vanilla UI templates"]
fn frame_cost_bench_changing_hud_bind_layout() {
    run_hud_bench("changing_hud", None);
}

/// `CINNABAR_HUD_PACK` names an unpacked server resource pack whose `ui/` overlays the HUD.
#[test]
#[ignore = "benchmark; needs the local vanilla UI templates and an unpacked server pack"]
fn frame_cost_bench_server_pack_hud() {
    let Some(pack) = std::env::var_os("CINNABAR_HUD_PACK") else {
        panic!(
            "requires the pinned local vanilla UI pack; fetch vanilla-assets first and CINNABAR_HUD_PACK"
        );
    };
    run_hud_bench("server_pack_hud", Some(PathBuf::from(pack)));
}

/// Time the original full-refresh workload, with optional incremental measurements.
fn run_hud_bench(name: &str, server_pack: Option<PathBuf>) {
    let vanilla = support::vanilla_pack().join("ui");
    assert!(
        vanilla.is_dir(),
        "fetch vanilla-assets before running the HUD benchmark"
    );
    let mut catalog = Catalog::load_dir(&vanilla).unwrap();
    let java = support::java_pack::files();
    catalog.apply_pack(
        java.iter()
            .map(|(path, bytes)| (path.as_str(), bytes.as_slice())),
    );
    if let Some(pack) = &server_pack {
        let files = pack_files(pack).expect("read requested benchmark pack");
        catalog.apply_pack(
            files
                .iter()
                .map(|(path, bytes)| (path.as_str(), bytes.as_slice())),
        );
    }
    let context = hud_context(&Context::desktop());
    let started = Instant::now();
    let tree = Arc::new(resolve(&catalog, HUD_SCREEN, &context).control.unwrap());
    let cold_resolve = started.elapsed();
    let cache = ResolveCache::default();
    let library = CachedLibrary {
        library: CatalogLibrary {
            catalog: &catalog,
            context: &context,
        },
        cache: &cache,
    };
    let env = LayoutEnv {
        text: &FixedText,
        textures: &FixedTextures,
    };
    let mut model = HudModel {
        survival_ui: true,
        hotbar_visible: true,
        xp_bar: true,
        chat_visible: true,
        chat: (0..50)
            .map(|index| Timed {
                text: format!("player {index}: busy server chat"),
                born: index as f64,
            })
            .collect(),
        sidebar: Some(Sidebar {
            title: "Busy server".into(),
            rows: (0..15)
                .map(|index| (format!("Player {index}"), index.to_string()))
                .collect(),
            background_opacity: 0.3,
            title_background_opacity: 0.4,
        }),
        boss_bars: (0..8)
            .map(|index| BossBar {
                name: format!("Boss {index}"),
                progress: 0.5,
                color: "#aa00aa".into(),
                notches: 0,
            })
            .collect(),
        ..HudModel::default()
    };
    let mut bind_time = std::time::Duration::ZERO;
    // The host keeps live binding state across refreshes.
    let mut state = BindState::new();
    let mut stateful_time = std::time::Duration::ZERO;
    let mut layout_time = std::time::Duration::ZERO;
    let incremental = std::env::var_os("CINNABAR_BENCH_INCREMENTAL").is_some();
    let mut incremental_time = std::time::Duration::ZERO;
    let stateful_only = std::env::var_os("CINNABAR_BENCH_STATEFUL_ONLY").is_some();
    let mut stats = frame_stats::FrameStats::default();
    let mut measures = json_ui::MeasureCache::default();
    let mut previous: Option<json_ui::FormRender> = None;
    let frames: u32 = std::env::var("CINNABAR_BENCH_FRAMES")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|frames| *frames > 0)
        .unwrap_or(200);
    for frame in 0..=frames {
        model.boss_bars[0].progress = f64::from(frame % 100) / 100.0;
        model.actionbar = Some(Timed {
            text: format!("Online: {} | Ping: {}ms", 200 + frame % 7, 40 + frame % 13),
            born: f64::from(frame),
        });
        let data = hud_data_source(&model);
        if stateful_only {
            stats.measure(|| {
                let mut updated = bind_stateful(&tree, &data, &library, &mut state).0;
                if let Some(old) = previous.take() {
                    let mut retained = old.bound;
                    measures.update_tree(&mut retained, updated);
                    updated = retained;
                }
                previous = Some(black_box(render_bound_cached(
                    updated,
                    [480.0, 270.0],
                    &env,
                    &ViewState::default(),
                    &mut measures,
                )));
            });
            continue;
        }
        let started = Instant::now();
        let bound = bind_shared(&tree, &data, &library);
        let bind_elapsed = started.elapsed();
        let started = Instant::now();
        let updated =
            incremental.then(|| black_box(bind_stateful(&tree, &data, &library, &mut state)).0);
        let stateful_elapsed = started.elapsed();
        let started = Instant::now();
        black_box(render_bound(
            bound,
            [480.0, 270.0],
            &env,
            &ViewState::default(),
        ));
        let layout_elapsed = started.elapsed();
        let started = Instant::now();
        if let Some(mut updated) = updated {
            if let Some(old) = previous.take() {
                let mut retained = old.bound;
                measures.update_tree(&mut retained, updated);
                updated = retained;
            }
            previous = Some(black_box(render_bound_cached(
                updated,
                [480.0, 270.0],
                &env,
                &ViewState::default(),
                &mut measures,
            )));
            if frame > 0 {
                incremental_time += started.elapsed();
            }
        }
        if frame > 0 {
            bind_time += bind_elapsed;
            stateful_time += stateful_elapsed;
            layout_time += layout_elapsed;
        }
    }
    if stateful_only {
        stats.report(name);
        return;
    }
    eprintln!(
        "FRAME_COST {name}: cold_resolve={:.3}ms bind={:.3}ms layout_emit={:.3}ms total={:.3}ms",
        cold_resolve.as_secs_f64() * 1e3,
        (bind_time / frames).as_secs_f64() * 1e3,
        (layout_time / frames).as_secs_f64() * 1e3,
        ((bind_time + layout_time) / frames).as_secs_f64() * 1e3
    );
    if incremental {
        eprintln!(
            "FRAME_INCREMENTAL {name}: bind={:.3}ms layout={:.3}ms total={:.3}ms",
            (stateful_time / frames).as_secs_f64() * 1e3,
            (incremental_time / frames).as_secs_f64() * 1e3,
            ((stateful_time + incremental_time) / frames).as_secs_f64() * 1e3
        );
    }
}

#[test]
fn review_missing_benchmark_pack_is_rejected() {
    let path = std::env::temp_dir().join(format!("missing-ui-bench-{}", std::process::id()));
    assert!(!path.exists());
    assert!(pack_files(&path).is_err());
}
