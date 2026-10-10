//! Optional release measurement of first-use and churn work with an installed carrier.

use crate::*;
use std::{sync::Arc, time::Instant};

#[test]
fn font_atlas_latency_fixture() {
    let Ok(path) = std::env::var("CINNABAR_FONT_SNAPSHOT_CARRIER") else {
        eprintln!(
            "skipping font_atlas_latency_fixture: missing CINNABAR_FONT_SNAPSHOT_CARRIER fixture"
        );
        return;
    };
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!(
                "skipping font_atlas_latency_fixture: font carrier fixture unavailable at {path} ({error})"
            );
            return;
        }
    };
    let manifest = assets::canonical_source_manifest_sha256(include_bytes!(
        "../../../../assets/cinnangles-sans-source.json"
    ));
    let font = Arc::new(assets::RuntimeFontCatalog::decode(&bytes, manifest).unwrap());
    let textures = Arc::new(
        UiTextureCatalog::new(
            (0..font.pages().len())
                .map(|index| UiTexturePage::font(Arc::clone(&font), index).unwrap())
                .collect(),
            font.pages().len(),
        )
        .unwrap(),
    );
    let mut atlas = FontAtlasFrame::default();
    let mut times = Vec::new();
    let mut upload_bytes = 0;
    for (frame, glyphs) in font.glyphs().chunks(128).enumerate() {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let mut batches = Vec::new();
        for glyph in glyphs {
            let [x0, y0, x1, y1] = glyph.uv.map(f32::from);
            let base = vertices.len() as u32;
            vertices.extend(
                [[x0, y0], [x1, y0], [x1, y1], [x0, y1]].map(|uv| UiRenderVertex {
                    position: [uv[0] - x0, uv[1] - y0],
                    uv,
                    clip_z: 0.,
                    clip_w: 1.,
                    color: [255; 4],
                    style_flags: 0,
                    alpha_cutoff: -1.,
                    model_light: 1.,
                    overlay_color: [0.; 4],
                }),
            );
            batches.push(UiRenderBatch::new(
                u32::from(glyph.page),
                UiScissor::new(0, 0, 1280, 720),
                indices.len() as u32,
                6,
                0,
            ));
            indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        let input = UiRenderInput {
            revision: frame as u64 + 1,
            viewport_size: [1280, 720],
            safe_area: [0; 4],
            vertices: vertices.into(),
            indices: indices.into(),
            batches: batches.into(),
            textures: Arc::clone(&textures),
        };
        input.validate().unwrap();
        let started = Instant::now();
        atlas
            .prepare(&input, |_, _, _, pixels| {
                upload_bytes += std::hint::black_box(pixels).len();
            })
            .unwrap();
        atlas.commit_vertices();
        times.push(started.elapsed().as_secs_f64() * 1000.0);
        atlas
            .prepare(&input, |_, _, _, _| panic!("a warm frame uploaded glyphs"))
            .unwrap();
        atlas.commit_vertices();
    }
    times.sort_by(f64::total_cmp);
    eprintln!(
        "FONT_ATLAS_CPU frames={} glyphs={} gpu_bytes={} upload_bytes={} p50_ms={:.4} p99_ms={:.4} max_ms={:.4}",
        times.len(),
        font.glyphs().len(),
        textures.plan().bytes(),
        upload_bytes,
        times[times.len() / 2],
        times[(times.len() * 99 / 100).min(times.len() - 1)],
        times[times.len() - 1]
    );
}
