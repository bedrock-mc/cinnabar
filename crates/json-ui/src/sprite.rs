//! The `image` control's sprite, following vanilla's draw dispatch: nine-slice first, then a clipped, tiled, filled (cover), kept-ratio
//! (contain) or stretched draw of the `uv`/`uv_size` source region. Source
//! coordinates are texture pixels until normalised into a [`UvRect`].

use serde_json::Value;

use crate::emit::{RectOut, SpriteQuad, UvRect};
use crate::layout::Rect;
use crate::sidecar::{NineSlice, TextureMeta};
use crate::tree::ResolvedControl;
use crate::widgets;

/// Most tiles one sprite may emit, bounding a tiny tile over a huge rect.
const MAX_TILES: usize = 4096;

/// `clip_direction`: which side of the image stays when `clip_ratio` cuts it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClipDirection {
    Left,
    Right,
    Up,
    Down,
    Center,
}

impl ClipDirection {
    /// `none`, an omitted value and anything unrecognised do not clip.
    pub(crate) fn of(control: &ResolvedControl) -> Option<Self> {
        match control.properties.get("clip_direction")?.as_str()? {
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            "up" => Some(Self::Up),
            "down" => Some(Self::Down),
            "center" => Some(Self::Center),
            _ => None,
        }
    }
}

/// `tiled`: which axes repeat. `true`, `"xy"` and `"yx"` repeat both.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tiled {
    X,
    Y,
    Both,
}

impl Tiled {
    fn of(control: &ResolvedControl) -> Option<Self> {
        match control.properties.get("tiled")? {
            Value::Bool(true) => Some(Self::Both),
            Value::String(axes) => match axes.as_str() {
                "x" => Some(Self::X),
                "y" => Some(Self::Y),
                "xy" | "yx" => Some(Self::Both),
                _ => None,
            },
            _ => None,
        }
    }
}

/// The source region and draw options an `image` resolves to.
struct Source {
    /// Texture pixel size, the space `uv`/`uv_size` address.
    texture: [f64; 2],
    uv: [f64; 2],
    uv_size: [f64; 2],
    nineslice: Option<(NineSlice, [f64; 2])>,
    tiled: Option<Tiled>,
    tiled_scale: [f64; 2],
}

impl Source {
    fn of(control: &ResolvedControl, meta: &TextureMeta) -> Self {
        let texture = meta.pixels;
        let uv = pair(control, "uv").unwrap_or([0.0, 0.0]);
        // A zero `uv_size` (the default) spans the whole texture.
        let uv_size = pair(control, "uv_size")
            .filter(|size| size[0] != 0.0 || size[1] != 0.0)
            .unwrap_or(texture);
        // A control's non-zero `nineslice_size` overrides the sidecar's; its
        // insets are in the sidecar's `base_size` units, scaled onto `uv_size`.
        let nineslice = control
            .properties
            .get("nineslice_size")
            .and_then(crate::sidecar::parse_nineslice)
            .filter(|inset| [inset.left, inset.top, inset.right, inset.bottom] != [0.0; 4])
            .or(meta.nineslice)
            .map(|inset| (inset, meta.base_size));
        Self {
            texture,
            uv,
            uv_size,
            nineslice,
            tiled: Tiled::of(control),
            tiled_scale: pair(control, "tiled_scale")
                .filter(|scale| scale[0] > 1e-6 && scale[1] > 1e-6)
                .unwrap_or([1.0, 1.0]),
        }
    }

    /// `[u, v, w, h]` in texture pixels, normalised.
    fn uv_rect(&self, region: [f64; 4]) -> UvRect {
        let [tw, th] = self.texture;
        UvRect {
            u0: (region[0] / tw) as f32,
            v0: (region[1] / th) as f32,
            u1: ((region[0] + region[2]) / tw) as f32,
            v1: ((region[1] + region[3]) / th) as f32,
        }
    }

    fn region(&self) -> [f64; 4] {
        [self.uv[0], self.uv[1], self.uv_size[0], self.uv_size[1]]
    }
}

/// The quads an `image` draws at `rect`; `clip_ratio` crops them toward its
/// `clip_direction`. `None` when the texture's size is unknown.
pub(crate) fn quads(
    control: &ResolvedControl,
    rect: Rect,
    meta: Option<&TextureMeta>,
    clip_ratio: Option<f32>,
) -> Vec<(Rect, UvRect)> {
    let Some(meta) = meta.filter(|meta| meta.pixels[0] > 0.0 && meta.pixels[1] > 0.0) else {
        return vec![(rect, UvRect::full())];
    };
    let source = Source::of(control, meta);
    let clip = ClipDirection::of(control)
        .zip(clip_ratio.filter(|ratio| *ratio > 0.0))
        .map(|(direction, ratio)| (direction, pixel_ratio(control, direction, ratio, &source)));
    let quads = if let Some((inset, base)) = source.nineslice {
        nine_slice_region(rect, &source, inset, base)
    } else if let Some(axes) = source.tiled {
        tiles(rect, &source, axes)
    } else if clip.is_some() {
        vec![(rect, source.uv_rect(source.region()))]
    } else if widgets::bound_bool(control, "fill") == Some(true) {
        vec![(rect, source.uv_rect(cover(rect, source.region())))]
    } else if widgets::bound_bool(control, "keep_ratio") != Some(false) {
        vec![(
            contain(rect, source.uv_size),
            source.uv_rect(source.region()),
        )]
    } else {
        vec![(rect, source.uv_rect(source.region()))]
    };
    let Some((direction, ratio)) = clip else {
        return quads;
    };
    let visible = clipped_rect(direction, rect, ratio);
    quads
        .into_iter()
        .filter_map(|(dest, uv)| crop(dest, uv, visible))
        .collect()
}

/// `clip_pixelperfect` (default on) rounds the cut down to whole source pixels
/// along the clipped axes.
fn pixel_ratio(
    control: &ResolvedControl,
    direction: ClipDirection,
    ratio: f32,
    source: &Source,
) -> [f64; 2] {
    let ratio = f64::from(ratio).min(1.0);
    let snap = |pixels: f64| {
        if widgets::bound_bool(control, "clip_pixelperfect") == Some(false) || pixels <= 0.0 {
            ratio
        } else {
            (ratio * pixels).floor() / pixels
        }
    };
    match direction {
        ClipDirection::Left | ClipDirection::Right => [snap(source.uv_size[0]), 0.0],
        ClipDirection::Up | ClipDirection::Down => [0.0, snap(source.uv_size[1])],
        ClipDirection::Center => [snap(source.uv_size[0]), snap(source.uv_size[1])],
    }
}

/// The part of `rect` left after cutting `ratio` of each axis away, pinned to
/// the side `direction` names.
fn clipped_rect(direction: ClipDirection, rect: Rect, ratio: [f64; 2]) -> Rect {
    let (cut_w, cut_h) = (rect.w * ratio[0], rect.h * ratio[1]);
    let (w, h) = (rect.w - cut_w, rect.h - cut_h);
    match direction {
        ClipDirection::Left | ClipDirection::Up => Rect::new(rect.x, rect.y, w, h),
        ClipDirection::Right | ClipDirection::Down => {
            Rect::new(rect.x + cut_w, rect.y + cut_h, w, h)
        }
        ClipDirection::Center => Rect::new(rect.x + cut_w * 0.5, rect.y + cut_h * 0.5, w, h),
    }
}

/// The part of `rect` a clip `ratio` keeps toward `direction` (a
/// `clip_direction` name); an unknown name keeps it whole.
pub(crate) fn clip_visible(rect: Rect, ratio: f32, direction: &str) -> Rect {
    let ratio = f64::from(ratio).clamp(0.0, 1.0);
    let (direction, cut) = match direction {
        "left" => (ClipDirection::Left, [ratio, 0.0]),
        "right" => (ClipDirection::Right, [ratio, 0.0]),
        "up" => (ClipDirection::Up, [0.0, ratio]),
        "down" => (ClipDirection::Down, [0.0, ratio]),
        "center" => (ClipDirection::Center, [ratio, ratio]),
        _ => return rect,
    };
    clipped_rect(direction, rect, cut)
}

/// Crop a quad to `visible`, scaling its UVs with its dest.
pub(crate) fn crop(dest: Rect, uv: UvRect, visible: Rect) -> Option<(Rect, UvRect)> {
    let kept = dest.intersect(visible);
    if kept.w <= 0.0 || kept.h <= 0.0 || dest.w <= 0.0 || dest.h <= 0.0 {
        return None;
    }
    let lerp_u = |x: f64| uv.u0 + (uv.u1 - uv.u0) * ((x - dest.x) / dest.w) as f32;
    let lerp_v = |y: f64| uv.v0 + (uv.v1 - uv.v0) * ((y - dest.y) / dest.h) as f32;
    Some((
        kept,
        UvRect {
            u0: lerp_u(kept.x),
            v0: lerp_v(kept.y),
            u1: lerp_u(kept.x + kept.w),
            v1: lerp_v(kept.y + kept.h),
        },
    ))
}

/// `keep_ratio`: the largest `uv_size`-shaped rect centred in `rect`.
fn contain(rect: Rect, uv_size: [f64; 2]) -> Rect {
    if uv_size[0] <= 0.0 || uv_size[1] <= 0.0 {
        return rect;
    }
    let (sx, sy) = ((rect.w / uv_size[0]) as f32, (rect.h / uv_size[1]) as f32);
    if (sx - sy).abs() <= f32::EPSILON {
        return rect;
    }
    let scale = f64::from(sx.min(sy));
    let (w, h) = (uv_size[0] * scale, uv_size[1] * scale);
    Rect::new(
        rect.x + (rect.w - w) * 0.5,
        rect.y + (rect.h - h) * 0.5,
        w,
        h,
    )
}

/// `fill`: the centred part of the source region shaped like `rect`.
fn cover(rect: Rect, [u, v, w, h]: [f64; 4]) -> [f64; 4] {
    if rect.w <= 0.0 || rect.h <= 0.0 || h <= 0.0 {
        return [u, v, w, h];
    }
    if w / h <= rect.w / rect.h {
        let cropped = rect.h / rect.w * w;
        [u, v + (h - cropped) * 0.5, w, cropped]
    } else {
        let cropped = rect.w / rect.h * h;
        [u + (w - cropped) * 0.5, v, cropped, h]
    }
}

/// Repeat `region` across `rect` from its top-left in `tiled_scale × uv_size`
/// steps, cropping the last tile; an untiled axis stretches.
fn tiles(rect: Rect, source: &Source, axes: Tiled) -> Vec<(Rect, UvRect)> {
    tile_region(rect, source, source.region(), source.uv_size, axes)
}

fn tile_region(
    rect: Rect,
    source: &Source,
    region: [f64; 4],
    tile: [f64; 2],
    axes: Tiled,
) -> Vec<(Rect, UvRect)> {
    if !rect.w.is_finite() || !rect.h.is_finite() || rect.w <= 0.0 || rect.h <= 0.0 {
        return Vec::new();
    }
    // Repeating one texel equals stretching it, and a wide 1x1 fill stays under MAX_TILES.
    let along_x = matches!(axes, Tiled::X | Tiled::Both) && region[2] > 1.0;
    let along_y = matches!(axes, Tiled::Y | Tiled::Both) && region[3] > 1.0;
    let axes = match (along_x, along_y) {
        (true, true) => Tiled::Both,
        (true, false) => Tiled::X,
        (false, true) => Tiled::Y,
        (false, false) => return vec![(rect, source.uv_rect(region))],
    };
    let step = [
        tile[0] * source.tiled_scale[0],
        tile[1] * source.tiled_scale[1],
    ];
    let step = match axes {
        Tiled::X => [step[0], rect.h],
        Tiled::Y => [rect.w, step[1]],
        Tiled::Both => step,
    };
    if step[0] <= 0.0 || step[1] <= 0.0 {
        return vec![(rect, source.uv_rect(region))];
    }
    let mut quads = Vec::new();
    let mut y = 0.0;
    let mut rows = 0;
    while y < rect.h && quads.len() < MAX_TILES && rows < MAX_TILES {
        rows += 1;
        let fy = ((rect.h - y) / step[1]).clamp(0.0, 1.0);
        let mut x = 0.0;
        while x < rect.w && quads.len() < MAX_TILES {
            let fx = ((rect.w - x) / step[0]).clamp(0.0, 1.0);
            quads.push((
                Rect::new(rect.x + x, rect.y + y, step[0] * fx, step[1] * fy),
                source.uv_rect([region[0], region[1], region[2] * fx, region[3] * fy]),
            ));
            x += step[0];
        }
        y += step[1];
    }
    quads
}

/// Nine-slice `rect` over the source region: corners 1:1, edges stretched (or
/// tiled on a `tiled` axis) along one axis, the centre along both.
fn nine_slice_region(
    rect: Rect,
    source: &Source,
    inset: NineSlice,
    base: [f64; 2],
) -> Vec<(Rect, UvRect)> {
    let [u, v, w, h] = source.region();
    let scale = [
        if base[0] > 0.0 { w / base[0] } else { 1.0 },
        if base[1] > 0.0 { h / base[1] } else { 1.0 },
    ];
    // A zero-size axis draws no border on that axis.
    let dest_inset = |a: f64, b: f64, size: f64| {
        if size == 0.0 {
            (0.0, 0.0)
        } else {
            fit(a, b, size)
        }
    };
    // Each cap stays inside its source region; opposing caps may still overlap.
    let source_inset =
        |inset: f64, scale: f64, size: f64| (inset * scale).max(0.0).min(size.max(0.0));
    let (src_l, src_r) = (
        source_inset(inset.left, scale[0], w),
        source_inset(inset.right, scale[0], w),
    );
    let (src_t, src_b) = (
        source_inset(inset.top, scale[1], h),
        source_inset(inset.bottom, scale[1], h),
    );
    let (dst_l, dst_r) = dest_inset(inset.left, inset.right, rect.w);
    let (dst_t, dst_b) = dest_inset(inset.top, inset.bottom, rect.h);
    let src_x = [0.0, src_l, w - src_r, w];
    let src_y = [0.0, src_t, h - src_b, h];
    let dst_x = [
        rect.x,
        rect.x + dst_l,
        rect.x + rect.w - dst_r,
        rect.x + rect.w,
    ];
    let dst_y = [
        rect.y,
        rect.y + dst_t,
        rect.y + rect.h - dst_b,
        rect.y + rect.h,
    ];
    let mut quads = Vec::with_capacity(9);
    for row in 0..3 {
        for col in 0..3 {
            let dest = Rect::new(
                dst_x[col],
                dst_y[row],
                dst_x[col + 1] - dst_x[col],
                dst_y[row + 1] - dst_y[row],
            );
            if dest.w <= 0.0 || dest.h <= 0.0 {
                continue;
            }
            // Overlapping source insets reverse the center's UVs. Meeting insets
            // sample the texel line there without changing either border.
            let (sx0, sx1) = texel_span(src_x[col], src_x[col + 1], w);
            let (sy0, sy1) = texel_span(src_y[row], src_y[row + 1], h);
            let piece = [u + sx0, v + sy0, sx1 - sx0, sy1 - sy0];
            let tiled = match source.tiled {
                Some(Tiled::Both) => Some(Tiled::Both),
                Some(Tiled::X) if col == 1 => Some(Tiled::X),
                Some(Tiled::Y) if row == 1 => Some(Tiled::Y),
                _ => None,
            }
            .filter(|_| col == 1 || row == 1);
            match tiled {
                Some(axes) => {
                    quads.extend(tile_region(dest, source, piece, [piece[2], piece[3]], axes))
                }
                None => quads.push((dest, source.uv_rect(piece))),
            }
        }
    }
    quads
}

/// Split `dest` into up to nine quads per `meta`'s nine-slice insets over the
/// whole texture; without insets a single full-texture quad.
pub fn nine_slice(dest: Rect, meta: &TextureMeta) -> Vec<SpriteQuad> {
    let source = Source {
        texture: meta.pixels,
        uv: [0.0, 0.0],
        uv_size: meta.pixels,
        nineslice: None,
        tiled: None,
        tiled_scale: [1.0, 1.0],
    };
    let quads = match meta.nineslice {
        Some(inset) if meta.pixels[0] > 0.0 && meta.pixels[1] > 0.0 => {
            nine_slice_region(dest, &source, inset, meta.base_size)
        }
        _ => vec![(dest, UvRect::full())],
    };
    quads
        .into_iter()
        .map(|(dest, uv)| SpriteQuad {
            dest: RectOut::from(dest),
            uv,
        })
        .collect()
}

/// Preserves reversed source spans; widens an empty span to one texel at its position.
fn texel_span(start: f64, end: f64, size: f64) -> (f64, f64) {
    if end != start {
        return (start, end);
    }
    let low = (start - 0.5).clamp(0.0, (size - 1.0).max(0.0));
    (low, (low + 1.0).min(size))
}

/// Two opposing insets clamped to a total; when they overflow they shrink in
/// proportion so the centre never goes negative.
fn fit(a: f64, b: f64, total: f64) -> (f64, f64) {
    let sum = a + b;
    if sum <= total || sum <= 0.0 {
        (a, b)
    } else {
        (a * total / sum, b * total / sum)
    }
}

fn pair(control: &ResolvedControl, key: &str) -> Option<[f64; 2]> {
    let items = control.properties.get(key)?.as_array()?;
    Some([items.first()?.as_f64()?, items.get(1)?.as_f64()?])
}

#[cfg(test)]
mod tests;
