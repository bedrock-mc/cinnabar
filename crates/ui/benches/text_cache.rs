use std::{hint::black_box, sync::Arc};

use assets::{CompiledFontCatalog, FontPixels, FontTexturePage, GlyphMetrics, encode_font_catalog};
use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use sha2::{Digest, Sha256};
use ui::{TextLayout, TextLayoutCache, TextLayoutRequest, TextStyle, UiScale};

/// Builds a small original atlas without requiring installed game assets.
fn font() -> CompiledFontCatalog {
    let pixels = vec![255; 4].into_boxed_slice();
    let page = FontTexturePage {
        source_path: "font/cache-bench.png".into(),
        source_bytes: pixels.len() as u32,
        source_sha256: [1; 32],
        pixels_sha256: Sha256::digest(&pixels).into(),
        width: 1,
        height: 1,
        pixels: FontPixels::Rgba8(pixels.into()),
    };
    let glyphs: Vec<_> = (' '..='~')
        .chain(['é', '…', '界', '\u{fffd}'])
        .map(|codepoint| GlyphMetrics {
            codepoint,
            page: 0,
            uv: [0, 0, 1, 1],
            bearing: [0, 0],
            advance_64: 64,
        })
        .collect();
    let encoded = encode_font_catalog([2; 32], &glyphs, &[page]).unwrap();
    CompiledFontCatalog::decode(&encoded, [2; 32]).unwrap()
}

/// Keeps dimensions and effects identical across all cache workloads.
fn request<'a>(text: &'a str, font: &'a CompiledFontCatalog) -> TextLayoutRequest<'a> {
    TextLayoutRequest {
        text,
        style: TextStyle::default(),
        width_64: 80 * 64,
        line_height_64: 64,
        baseline_64: 0,
        scale: UiScale::new(1.0).unwrap(),
        font,
        wrap: Default::default(),
    }
}

/// Compares every published layout field except its cache-local sequence number.
fn assert_same(left: &TextLayout, right: &TextLayout) {
    assert_eq!(left.key(), right.key());
    assert_eq!(left.glyphs(), right.glyphs());
    assert_eq!(left.glyph_source_indices(), right.glyph_source_indices());
    assert_eq!(left.line_count(), right.line_count());
    assert_eq!(left.size_64(), right.size_64());
    assert_eq!(left.ellipsized(), right.ellipsized());
    assert_eq!(left.linear_sampling(), right.linear_sampling());
    assert_eq!(left.rendering(), right.rendering());
}

/// Measures short and long hits, single-line hits, and churn at two cache sizes.
fn text_cache(c: &mut Criterion) {
    let font = font();
    let mut group = c.benchmark_group("text_cache");
    group.sample_size(30);
    for (name, text) in [
        ("hit_only", "§lScore: §r12345 é界?".to_owned()),
        ("long_string", "Formatted §cchat§r é界? ".repeat(384)),
    ] {
        let mut cache = TextLayoutCache::new(1024, 64 * 1024 * 1024);
        let request = request(&text, &font);
        let expected = TextLayoutCache::new(0, 0).layout(request).unwrap();
        let first = cache.layout(request).unwrap();
        assert_same(&first, &expected);
        assert!(Arc::ptr_eq(&first, &cache.layout(request).unwrap()));
        group.bench_function(name, |b| {
            b.iter(|| black_box(cache.layout(black_box(request)).unwrap()));
        });
    }
    let single_request = request("  A\nB §cé界? text that needs ellipsis   ", &font);
    let mut cache = TextLayoutCache::new(1024, 64 * 1024 * 1024);
    let expected = TextLayoutCache::new(0, 0)
        .single_line(single_request, 8 * 64)
        .unwrap();
    assert_same(
        &cache.single_line(single_request, 8 * 64).unwrap(),
        &expected,
    );
    group.bench_function("single_line_hit", |b| {
        b.iter(|| {
            black_box(
                cache
                    .single_line(black_box(single_request), 8 * 64)
                    .unwrap(),
            )
        });
    });
    for capacity in [16, 1024] {
        let texts: Vec<_> = (0..=capacity)
            .map(|index| format!("chat {index:04}"))
            .collect();
        let mut cache = TextLayoutCache::new(capacity, 64 * 1024 * 1024);
        for text in &texts[..capacity] {
            let request = request(text, &font);
            let expected = TextLayoutCache::new(0, 0).layout(request).unwrap();
            assert_same(&cache.layout(request).unwrap(), &expected);
        }
        let mut index = capacity;
        group.bench_with_input(BenchmarkId::new("eviction", capacity), &capacity, |b, _| {
            b.iter(|| {
                let layout = cache
                    .layout(request(black_box(&texts[index]), &font))
                    .unwrap();
                index = (index + 1) % texts.len();
                black_box(layout)
            });
        });
    }
    group.finish();
}

criterion_group!(benches, text_cache);
criterion_main!(benches);
