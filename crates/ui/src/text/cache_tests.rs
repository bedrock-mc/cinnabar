use super::*;
use assets::{FontPixels, FontTexturePage, GlyphMetrics, encode_font_catalog};

/// Builds a self-contained atlas with an explicit replacement glyph.
fn font() -> CompiledFontCatalog {
    font_with_identity([2; 32])
}

/// Gives otherwise identical metrics a distinct carrier identity.
fn font_with_identity(identity: [u8; 32]) -> CompiledFontCatalog {
    let pixels = vec![255; 4].into_boxed_slice();
    let page = FontTexturePage {
        source_path: "font/cache-test.png".into(),
        source_bytes: 4,
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
    let encoded = encode_font_catalog(identity, &glyphs, &[page]).unwrap();
    CompiledFontCatalog::decode(&encoded, identity).unwrap()
}

/// Uses fixed metrics so only text and cache state vary in cache regressions.
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

#[test]
fn warm_hits_do_not_run_sha256() {
    let font = font();
    let text = "Formatted §cchat§r é界? ".repeat(384);
    let request = request(&text, &font);
    let mut cache = TextLayoutCache::new(128, 8 * 1024 * 1024);
    let first = cache.layout(request).unwrap();
    let sha256_calls = cache.work.sha256_calls;
    for _ in 0..64 {
        assert!(Arc::ptr_eq(&first, &cache.layout(request).unwrap()));
    }
    assert_eq!(
        cache.work.sha256_calls - sha256_calls,
        0,
        "warm SHA-256 calls"
    );
    let first = cache.single_line(request, 64).unwrap();
    let sha256_calls = cache.work.sha256_calls;
    for _ in 0..64 {
        assert!(Arc::ptr_eq(
            &first,
            &cache.single_line(request, 64).unwrap()
        ));
    }
    assert_eq!(
        cache.work.sha256_calls - sha256_calls,
        0,
        "single-line SHA-256 calls"
    );
}

#[test]
fn eviction_selects_one_candidate_at_any_capacity() {
    let font = font();
    for capacity in [16, 1024] {
        let mut cache = TextLayoutCache::new(capacity, 64 * 1024 * 1024);
        for index in 0..capacity {
            cache
                .layout(request(&format!("chat {index}"), &font))
                .unwrap();
        }
        let before = cache.work.eviction_candidates;
        cache.layout(request("next chat", &font)).unwrap();
        assert_eq!(cache.len(), capacity);
        assert_eq!(
            cache.work.eviction_candidates - before,
            1,
            "eviction candidates at capacity {capacity}"
        );
    }
}

/// Compares all output, including the published digest, independently of cache IDs.
fn assert_same_output(left: &TextLayout, right: &TextLayout) {
    let mut left = left.clone();
    left.id = right.id;
    assert_eq!(&left, right);
}

#[test]
fn hash_collisions_never_reuse_another_texts_layout() {
    let font = font();
    let mut cache = TextLayoutCache::new(8, 1024 * 1024);
    cache.work.forced_content_hash = Some(0);
    for text in ["A", "B", "A", "é界?", "B"] {
        let request = request(text, &font);
        let expected = TextLayoutCache::new(0, 0).layout(request).unwrap();
        let layout = cache.layout(request).unwrap();
        assert_same_output(&layout, &expected);
        assert!(Arc::ptr_eq(&layout, &cache.layout(request).unwrap()));
        assert!(cache.len() <= 2);
        let expected = TextLayoutCache::new(0, 0).single_line(request, 64).unwrap();
        assert_same_output(&cache.single_line(request, 64).unwrap(), &expected);
    }
}

#[test]
fn cached_output_matches_uncached_wrapping_fallback_effects_and_ellipsis() {
    let fonts = [font(), font_with_identity([3; 32])];
    let mut cache = TextLayoutCache::new(128, 8 * 1024 * 1024);
    for font in &fonts {
        for text in ["", "A§lB§r é界?\nC", "A\t  B é界?", "unknown 🦀"] {
            for scale in [1.0, 1.5, 2.0] {
                let request = TextLayoutRequest {
                    scale: UiScale::new(scale).unwrap(),
                    style: TextStyle {
                        italic: true,
                        ..Default::default()
                    },
                    wrap: TextWrap {
                        align: TextLineAlign::Center,
                        letter_spacing_64: 1,
                        line_padding_64: 2,
                        allow_visual_overflow: true,
                        max_lines: Some(3),
                        device_scale_65536: 65536,
                        snap_glyphs_to_device_pixels: true,
                        ..Default::default()
                    },
                    width_64: 4 * 64,
                    ..request(text, font)
                };
                let expected = TextLayoutCache::new(0, 0).layout(request).unwrap();
                assert_same_output(&cache.layout(request).unwrap(), &expected);
                assert_same_output(&cache.layout(request).unwrap(), &expected);
                let expected = TextLayoutCache::new(0, 0)
                    .single_line(request, 2 * 64)
                    .unwrap();
                assert_same_output(&cache.single_line(request, 2 * 64).unwrap(), &expected);
                assert_same_output(&cache.single_line(request, 2 * 64).unwrap(), &expected);
            }
        }
    }
}

#[test]
fn lru_order_and_retained_bytes_match_a_reference_trace() {
    let font = font();
    for capacity in [1, 2, 7, 16] {
        let mut cache = TextLayoutCache::new(capacity, 1024 * 1024);
        let mut order: Vec<(usize, u64)> = Vec::new();
        let mut sequence = 7u32;
        for _ in 0..2048 {
            sequence = sequence.wrapping_mul(1664525).wrapping_add(1013904223);
            let index = sequence as usize % 23;
            let layout = cache
                .layout(request(&format!("chat {index:02}"), &font))
                .unwrap();
            if let Some(at) = order.iter().position(|(text, _)| *text == index) {
                let (_, id) = order.remove(at);
                assert_eq!(layout.id(), id);
            } else if order.len() == capacity {
                order.remove(0);
            }
            order.push((index, layout.id()));
            assert_eq!(cache.len(), order.len());
            assert_eq!(
                cache.retained_bytes(),
                cache
                    .entries
                    .values()
                    .map(|entry| entry.retained_bytes)
                    .sum::<usize>()
            );
            let mut chain = Vec::new();
            let mut next = cache.oldest.as_ref();
            while let Some(key) = next {
                let entry = &cache.entries[key];
                chain.push(entry.layout.id());
                assert!(chain.len() <= cache.len(), "LRU chain must not cycle");
                next = entry.next.as_ref();
            }
            assert_eq!(chain, order.iter().map(|(_, id)| *id).collect::<Vec<_>>());
        }
    }
}

#[test]
fn byte_pressure_can_evict_multiple_entries_and_charges_raw_single_line_text() {
    let font = font();
    let mut probe = TextLayoutCache::new(8, usize::MAX);
    probe.layout(request("A", &font)).unwrap();
    let small = probe.retained_bytes();
    let large_text = "A".repeat(256);
    let mut large_probe = TextLayoutCache::new(1, usize::MAX);
    large_probe.layout(request(&large_text, &font)).unwrap();
    let large = large_probe.retained_bytes();
    assert!(large > small);
    let cap = large.max(small * 3);
    let mut cache = TextLayoutCache::new(8, cap);
    for text in ["A", "B", "C"] {
        cache.layout(request(text, &font)).unwrap();
    }
    let before = cache.len();
    cache.layout(request(&large_text, &font)).unwrap();
    assert!(
        cache.len() < before,
        "larger layouts must evict multiple entries"
    );
    assert!(cache.retained_bytes() <= cap);
    let raw = " ".repeat(cap * 2);
    cache.single_line(request(&raw, &font), 64).unwrap();
    let before = cache.built_layout_count();
    cache.single_line(request(&raw, &font), 64).unwrap();
    assert!(cache.retained_bytes() <= cap);
    assert!(
        !cache
            .entries
            .values()
            .any(|entry| entry.text.as_ref() == raw)
    );
    assert_eq!(cache.built_layout_count(), before);
}

#[test]
fn failures_and_counter_overflow_leave_retention_intact() {
    let font = font();
    let mut cache = TextLayoutCache::new(1, 1024 * 1024);
    let first = cache.layout(request("A", &font)).unwrap();
    let invalid = TextLayoutRequest {
        width_64: 0,
        ..request("B", &font)
    };
    assert!(matches!(
        cache.layout(invalid),
        Err(TextError::ZeroWrapWidth)
    ));
    assert!(Arc::ptr_eq(
        &first,
        &cache.layout(request("A", &font)).unwrap()
    ));
    cache.clock = u64::MAX;
    assert!(matches!(
        cache.layout(request("A", &font)),
        Err(TextError::CacheCounterOverflow)
    ));
    cache.clock = 0;
    cache.next_id = u64::MAX;
    assert!(matches!(
        cache.layout(request("B", &font)),
        Err(TextError::CacheCounterOverflow)
    ));
    assert!(Arc::ptr_eq(
        &first,
        &cache.layout(request("A", &font)).unwrap()
    ));
}
