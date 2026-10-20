use serde_json::{Value, json};

use super::*;
use crate::sidecar::NineSlice;

fn image(properties: Value) -> ResolvedControl {
    ResolvedControl {
        name: "image".into(),
        control_type: Some("image".into()),
        properties: properties
            .as_object()
            .unwrap()
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
        base: None,
        unresolved_base: None,
        children: Vec::new(),
        factory: None,
    }
}

fn wide() -> TextureMeta {
    TextureMeta::plain([16.0, 8.0])
}

fn frame() -> TextureMeta {
    TextureMeta {
        nineslice: Some(NineSlice {
            left: 4.0,
            top: 4.0,
            right: 4.0,
            bottom: 4.0,
        }),
        ..TextureMeta::plain([16.0, 16.0])
    }
}

fn draw(properties: Value, size: [f64; 2], meta: TextureMeta) -> Vec<Quad> {
    quads(
        &image(properties),
        Rect::new(0.0, 0.0, size[0], size[1]),
        Some(&meta),
        None,
    )
}

fn uv(u0: f32, v0: f32, u1: f32, v1: f32) -> UvRect {
    UvRect { u0, v0, u1, v1 }
}

// `keep_ratio` defaults on: a 16x8 texture in a 16x16 rect letterboxes.
#[test]
fn default_keep_ratio_contains_the_texture() {
    let quads = draw(json!({}), [16.0, 16.0], wide());
    assert_eq!(
        quads,
        vec![(Rect::new(0.0, 4.0, 16.0, 8.0), UvRect::full(), None)]
    );
    let stretched = draw(json!({ "keep_ratio": false }), [16.0, 16.0], wide());
    assert_eq!(stretched[0].0, Rect::new(0.0, 0.0, 16.0, 16.0));
}

#[test]
fn scalar_sidecar_size_keeps_panel_borders_at_their_authored_width() {
    let mut meta = crate::sidecar::parse_texture_meta(&json!({
        "base_size": 3,
        "nineslice_size": 1
    }))
    .expect("a square sidecar keeps its nine-slice metadata");
    meta.pixels = [3.0, 3.0];
    let quads = draw(json!({}), [120.0, 40.0], meta);
    assert_eq!(quads.len(), 9);
    assert_eq!(quads[0].0, Rect::new(0.0, 0.0, 1.0, 1.0));
    assert_eq!(quads[8].0, Rect::new(119.0, 39.0, 1.0, 1.0));
}

#[test]
fn numeric_sidecar_size_keeps_source_pixel_borders() {
    let mut meta = crate::sidecar::parse_texture_meta(&json!({
        "base_size": 60,
        "nineslice_size": 1
    }))
    .unwrap();
    meta.pixels = [3.0, 3.0];
    let quads = draw(json!({}), [120.0, 40.0], meta);
    assert_eq!(quads[0].1.u1, 1.0 / 3.0);
    assert_eq!(quads[0].1.v1, 1.0 / 3.0);
}

// `fill` covers the rect, cropping the source to x 4..12.
#[test]
fn fill_crops_the_source_to_cover() {
    let quads = draw(json!({ "fill": true }), [16.0, 16.0], wide());
    assert_eq!(
        quads,
        vec![(
            Rect::new(0.0, 0.0, 16.0, 16.0),
            uv(0.25, 0.0, 0.75, 1.0),
            None
        )]
    );
}

// `uv` alone keeps the texture-sized default `uv_size`; `uv_size` alone starts at 0.
#[test]
fn uv_and_uv_size_default_independently() {
    let offset = draw(
        json!({ "uv": [4, 0], "keep_ratio": false }),
        [8.0, 8.0],
        wide(),
    );
    assert_eq!(offset[0].1, uv(0.25, 0.0, 1.25, 1.0));
    let region = draw(
        json!({ "uv_size": [8, 8], "keep_ratio": false }),
        [8.0, 8.0],
        wide(),
    );
    assert_eq!(region[0].1, uv(0.0, 0.0, 0.5, 1.0));
}

// A control `nineslice_size` slices a texture without a sidecar slice.
#[test]
fn control_nineslice_overrides_the_sidecar() {
    let quads = draw(
        json!({ "nineslice_size": 4 }),
        [40.0, 40.0],
        TextureMeta::plain([16.0, 16.0]),
    );
    assert_eq!(quads.len(), 9);
    assert_eq!(quads[0].0, Rect::new(0.0, 0.0, 4.0, 4.0));
    let edges = draw(
        json!({ "nineslice_size": [1, 2, 3, 4] }),
        [40.0, 40.0],
        TextureMeta::plain([16.0, 16.0]),
    );
    assert_eq!(edges[8].0, Rect::new(37.0, 36.0, 3.0, 4.0));
}

// `"yx"` tiles both axes like `"xy"`.
#[test]
fn tiled_yx_repeats_both_axes() {
    let tile = TextureMeta::plain([8.0, 8.0]);
    assert_eq!(draw(json!({ "tiled": "yx" }), [32.0, 32.0], tile).len(), 1);
    assert_eq!(
        draw(json!({ "tiled": "true" }), [32.0, 32.0], tile).len(),
        1
    );
}

// `tiled_scale` scales the tile step.
#[test]
fn tiled_scale_scales_each_tile() {
    let quads = draw(
        json!({ "tiled": true, "tiled_scale": [2, 2] }),
        [32.0, 32.0],
        TextureMeta::plain([8.0, 8.0]),
    );
    assert_eq!(quads.len(), 1);
    assert_eq!(quads[0].2.unwrap().uv, uv(0.0, 0.0, 2.0, 2.0));
}

// Tiling repeats the `uv` region, not the whole texture.
#[test]
fn tiling_repeats_the_uv_region() {
    let quads = draw(
        json!({ "tiled": true, "uv": [0, 0], "uv_size": [8, 8] }),
        [32.0, 32.0],
        TextureMeta::plain([16.0, 16.0]),
    );
    assert_eq!(quads.len(), 1);
    assert!(quads.iter().all(|(_, uv, _)| uv.u1 == 0.5 && uv.v1 == 0.5));
}

// A tiled 1x1 fill (vanilla `dropDownSelectBG`) covers a wide row whole, not up to the tile cap.
#[test]
fn tiled_single_texel_stretches_across_the_rect() {
    let rect = [400.0, 19.0];
    let quads = draw(
        json!({ "tiled": true }),
        rect,
        TextureMeta::plain([1.0, 1.0]),
    );
    assert_eq!(
        quads,
        vec![(
            Rect::new(0.0, 0.0, 400.0, 19.0),
            uv(0.0, 0.0, 1.0, 1.0),
            None
        )]
    );
    let strip = draw(
        json!({ "tiled": true }),
        [32.0, 32.0],
        TextureMeta::plain([8.0, 1.0]),
    );
    assert_eq!(strip.len(), 1);
    assert_eq!(strip[0].2.unwrap().axes, [true, false]);
    assert!(strip.iter().all(|(dest, _, _)| dest.h == 32.0));
}

// Tiling a nine-slice texture keeps its corners.
#[test]
fn tiled_nine_slice_keeps_the_border() {
    let quads = draw(json!({ "tiled": true }), [40.0, 40.0], frame());
    assert_eq!(
        quads[0],
        (
            Rect::new(0.0, 0.0, 4.0, 4.0),
            uv(0.0, 0.0, 0.25, 0.25),
            None
        )
    );
    assert_eq!(quads.len(), 9);
}

/// Checks pixel coverage and samples a two-color source without relying on quad counts.
fn checker_pixels(quads: &[Quad], size: [usize; 2], texture: [f64; 2]) -> Vec<u8> {
    let mut pixels = vec![0; size[0] * size[1]];
    for (dest, uv, repeat) in quads {
        for y in dest.y.floor().max(0.0) as usize..((dest.y + dest.h).ceil() as usize).min(size[1])
        {
            for x in
                dest.x.floor().max(0.0) as usize..((dest.x + dest.w).ceil() as usize).min(size[0])
            {
                let at = [x as f64 + 0.5, y as f64 + 0.5];
                if at[0] < dest.x
                    || at[0] >= dest.x + dest.w
                    || at[1] < dest.y
                    || at[1] >= dest.y + dest.h
                {
                    continue;
                }
                let coords = repeat.map_or(*uv, |r| r.uv);
                let mut u = f64::from(coords.u0)
                    + f64::from(coords.u1 - coords.u0) * (at[0] - dest.x) / dest.w;
                let mut v = f64::from(coords.v0)
                    + f64::from(coords.v1 - coords.v0) * (at[1] - dest.y) / dest.h;
                if let Some(repeat) = repeat {
                    if repeat.axes[0] {
                        u = u.rem_euclid(1.0);
                    }
                    if repeat.axes[1] {
                        v = v.rem_euclid(1.0);
                    }
                    u = f64::from(uv.u0) + u * f64::from(uv.u1 - uv.u0);
                    v = f64::from(uv.v0) + v * f64::from(uv.v1 - uv.v0);
                }
                let pixel = &mut pixels[y * size[0] + x];
                assert_eq!(*pixel, 0, "overlapping tiles at {x},{y}");
                *pixel = 1
                    + (((u * texture[0]).floor() as usize + (v * texture[1]).floor() as usize) % 2)
                        as u8;
            }
        }
    }
    pixels
}

#[test]
fn large_tiled_images_cover_every_pixel_and_crop_edge_uvs() {
    for (axes, size) in [("xy", [257, 131]), ("x", [16_387, 3]), ("y", [3, 16_387])] {
        let quads = draw(
            json!({"tiled": axes, "uv": [2, 2], "uv_size": [2, 2], "tiled_scale": [2, 1]}),
            size.map(|value| value as f64),
            TextureMeta::plain([8.0, 8.0]),
        );
        let pixels = checker_pixels(&quads, size, [8.0, 8.0]);
        for y in 0..size[1] {
            for x in 0..size[0] {
                let u = if axes == "y" {
                    (2 * x + 1) / size[0]
                } else {
                    (x % 4) / 2
                };
                let v = if axes == "x" {
                    (2 * y + 1) / size[1]
                } else {
                    y % 2
                };
                assert_eq!(
                    pixels[y * size[0] + x],
                    1 + ((u + v) % 2) as u8,
                    "{axes} at {x},{y}"
                );
            }
        }
    }
}

#[test]
fn tiled_geometry_is_bounded_even_at_extreme_scales() {
    // The ordinary case fails before the extreme case can allocate tile geometry.
    for scale in [1.0, 0.001] {
        let quads = draw(
            json!({"tiled": true, "tiled_scale": [scale, scale]}),
            [256.0, 128.0],
            TextureMeta::plain([2.0, 2.0]),
        );
        assert_eq!(quads.len(), 1, "repeat scale {scale} must use one quad");
        let span = quads[0].2.unwrap().uv;
        assert!((f64::from(span.u1) - 128.0 / scale).abs() < 0.01);
        assert!((f64::from(span.v1) - 64.0 / scale).abs() < 0.01);
        let pixels = checker_pixels(&quads, [256, 128], [2.0, 2.0]);
        assert!(pixels.iter().all(|pixel| *pixel != 0));
    }
    let slices = draw(
        json!({"tiled": true, "tiled_scale": [0.001, 0.001], "nineslice_size": 1}),
        [256.0, 128.0],
        TextureMeta::plain([4.0, 4.0]),
    );
    assert_eq!(slices.len(), 9);
    assert!(
        checker_pixels(&slices, [256, 128], [4.0, 4.0])
            .iter()
            .all(|pixel| *pixel != 0)
    );
}

#[test]
fn large_tiled_slices_keep_full_coverage_and_clipped_borders() {
    let size = [267, 139];
    let control = image(json!({"tiled": true, "nineslice_size": 1, "clip_direction": "down"}));
    let meta = TextureMeta::plain([4.0, 4.0]);
    for clip in [None, Some(0.5)] {
        let quads = quads(
            &control,
            Rect::new(0.0, 0.0, size[0] as f64, size[1] as f64),
            Some(&meta),
            clip,
        );
        let pixels = checker_pixels(&quads, size, meta.pixels);
        for y in 0..size[1] {
            for x in 0..size[0] {
                if clip.is_some() && y < size[1] / 2 {
                    assert_eq!(pixels[y * size[0] + x], 0);
                    continue;
                }
                let u = if x == 0 {
                    0
                } else if x + 1 == size[0] {
                    3
                } else {
                    1 + (x - 1) % 2
                };
                let v = if y == 0 {
                    0
                } else if y + 1 == size[1] {
                    3
                } else {
                    1 + (y - 1) % 2
                };
                assert_eq!(
                    pixels[y * size[0] + x],
                    1 + ((u + v) % 2) as u8,
                    "clip={clip:?} at {x},{y}"
                );
            }
        }
    }
}

// Without `clip_direction` a `clip_ratio` cuts nothing; `none` is the same.
#[test]
fn omitted_clip_direction_does_not_clip() {
    let icon = TextureMeta::plain([16.0, 16.0]);
    for properties in [json!({}), json!({ "clip_direction": "none" })] {
        let quads = quads(
            &image(properties),
            Rect::new(0.0, 0.0, 16.0, 16.0),
            Some(&icon),
            Some(0.5),
        );
        assert_eq!(quads[0].0.w, 16.0);
    }
}

// `clip_pixelperfect` (default) removes floor(0.3 × 7) = 2 of 7 source pixels.
#[test]
fn pixel_perfect_clip_snaps_to_source_pixels() {
    let seven = TextureMeta::plain([7.0, 1.0]);
    let clip = |extra: Value| {
        let mut properties = json!({ "clip_direction": "left" });
        properties
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        quads(
            &image(properties),
            Rect::new(0.0, 0.0, 70.0, 10.0),
            Some(&seven),
            Some(0.3),
        )[0]
    };
    let (rect, uv, _) = clip(json!({}));
    assert_eq!(rect.w, 50.0);
    assert!((uv.u1 - 5.0 / 7.0).abs() < 1e-6);
    let (rect, _, _) = clip(json!({ "clip_pixelperfect": false }));
    assert!((rect.w - 49.0).abs() < 1e-4);
}

// A clipped image draws its whole region into the rect, not kept to ratio.
#[test]
fn clipped_images_skip_keep_ratio() {
    let quads = quads(
        &image(json!({ "clip_direction": "right" })),
        Rect::new(0.0, 0.0, 16.0, 16.0),
        Some(&wide()),
        Some(0.25),
    );
    assert_eq!(quads[0].0, Rect::new(4.0, 0.0, 12.0, 16.0));
}

#[test]
fn plain_texture_is_one_full_quad() {
    let quads = nine_slice(
        Rect::new(0.0, 0.0, 100.0, 50.0),
        &TextureMeta::plain([64.0; 2]),
    );
    assert_eq!(quads.len(), 1);
    assert_eq!(quads[0].uv, UvRect::full());
}

// top=0 removes the whole top row: 6 quads, not 9.
#[test]
fn zero_edge_collapses_its_row() {
    let meta = TextureMeta {
        nineslice: Some(NineSlice {
            left: 1.0,
            top: 0.0,
            right: 1.0,
            bottom: 1.0,
        }),
        ..TextureMeta::plain([3.0, 2.0])
    };
    let quads = nine_slice(Rect::new(0.0, 0.0, 30.0, 20.0), &meta);
    assert_eq!(quads.len(), 6);
    assert_eq!(quads[0].dest.y, 0.0);
}

// A 2x2 texture sliced at 1 still fills its centre.
#[test]
fn meeting_insets_stretch_the_middle_texel() {
    let meta = TextureMeta {
        nineslice: Some(NineSlice {
            left: 1.0,
            top: 1.0,
            right: 1.0,
            bottom: 1.0,
        }),
        ..TextureMeta::plain([2.0, 2.0])
    };
    let quads = nine_slice(Rect::new(0.0, 0.0, 30.0, 9.0), &meta);
    assert_eq!(quads.len(), 9);
    let centre = quads[4];
    assert_eq!([centre.dest.w, centre.dest.h], [28.0, 7.0]);
    assert_eq!([centre.uv.u0, centre.uv.u1], [0.25, 0.75]);
}

/// Returns whether nearest-filtered quads sample the original frame's center texel.
fn frame_pixel(quads: &[SpriteQuad], at: [f64; 2]) -> bool {
    let quad = quads
        .iter()
        .find(|q| {
            at[0] >= q.dest.x
                && at[0] < q.dest.x + q.dest.w
                && at[1] >= q.dest.y
                && at[1] < q.dest.y + q.dest.h
        })
        .expect("the frame covers every destination pixel");
    let u = f64::from(quad.uv.u0)
        + f64::from(quad.uv.u1 - quad.uv.u0) * (at[0] - quad.dest.x) / quad.dest.w;
    let v = f64::from(quad.uv.v0)
        + f64::from(quad.uv.v1 - quad.uv.v0) * (at[1] - quad.dest.y) / quad.dest.h;
    (u * 3.0).floor() == 1.0 && (v * 3.0).floor() == 1.0
}

#[test]
fn overlapping_source_slices_keep_a_tiny_disabled_frames_border_one_texel_wide() {
    let meta = TextureMeta {
        nineslice: Some(NineSlice {
            left: 2.0,
            top: 2.0,
            right: 2.0,
            bottom: 2.0,
        }),
        ..TextureMeta::plain([3.0, 3.0])
    };
    let quads = nine_slice(Rect::new(0.0, 0.0, 40.0, 20.0), &meta);
    for at in [
        [1.1, 10.0],
        [38.9, 10.0],
        [20.0, 1.1],
        [20.0, 18.9],
        [20.0, 10.0],
    ] {
        assert!(
            frame_pixel(&quads, at),
            "{at:?} must sample the frame's face"
        );
    }
    for at in [[0.9, 10.0], [39.1, 10.0], [20.0, 0.9], [20.0, 19.1]] {
        assert!(
            !frame_pixel(&quads, at),
            "{at:?} must sample the frame's border"
        );
    }
}

#[test]
fn oversized_source_insets_do_not_sample_outside_the_texture() {
    let meta = TextureMeta {
        nineslice: Some(NineSlice {
            left: 5.0,
            top: 5.0,
            right: 5.0,
            bottom: 5.0,
        }),
        ..TextureMeta::plain([3.0, 3.0])
    };
    for quad in nine_slice(Rect::new(0.0, 0.0, 40.0, 20.0), &meta) {
        for edge in [quad.uv.u0, quad.uv.v0, quad.uv.u1, quad.uv.v1] {
            assert!(
                (0.0..=1.0).contains(&edge),
                "sampled outside the texture: {edge}"
            );
        }
    }
}

// Insets are in `base_size` units: a 10px texture with base 5 samples 2×2 px borders.
#[test]
fn insets_scale_from_base_size_onto_pixels() {
    let meta = TextureMeta {
        base_size: [5.0, 5.0],
        nineslice: Some(NineSlice {
            left: 2.0,
            top: 2.0,
            right: 2.0,
            bottom: 2.0,
        }),
        pixels: [10.0, 10.0],
    };
    let quads = nine_slice(Rect::new(0.0, 0.0, 20.0, 20.0), &meta);
    assert_eq!(quads[0].dest.w, 2.0);
    assert_eq!(quads[0].uv.u1, 0.4);
}

#[test]
fn empty_tiled_destinations_emit_nothing() {
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let source = Source {
            texture: [16.0, 16.0],
            uv: [0.0, 0.0],
            uv_size: [16.0, 16.0],
            nineslice: None,
            tiled: Some(Tiled::Both),
            tiled_scale: [1.0, 1.0],
        };
        for [w, h] in [[0.0, 1.0], [-1.0, 1.0], [1.0, 0.0]] {
            let mut quads = Vec::new();
            tile_region(
                Rect::new(0.0, 0.0, w, h),
                &source,
                source.region(),
                [1.0, 1e-12],
                Tiled::Both,
                &mut quads,
            );
            assert!(quads.is_empty());
        }
        send.send(()).unwrap();
    });
    receive
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("empty sprites must finish without traversing rows");
}
