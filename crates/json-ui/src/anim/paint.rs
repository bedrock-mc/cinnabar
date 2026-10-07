//! What animations do to one draw at paint time: the animated controls that
//! write its properties, fade it through `propagate_alpha`, or move it with
//! their offset and size, and the draw they produce under an [`Animator`].

use std::collections::BTreeMap;
use std::sync::Arc;

use super::def::{AnimGraph, AnimKind};
use super::runtime::{Animator, Written};
use crate::emit::{Draw, DrawNode, RectOut, UvRect};
use crate::layout::{Rect, TextureSource};

/// Flip-book frames sit this far inside their cell, in texture pixels.
const FRAME_INSET: f32 = 0.00390625;
/// And are this much smaller than it.
const FRAME_SHRINK: f32 = -0.0078125;

/// One animated control as laid out.
#[derive(Clone, Debug, PartialEq)]
pub struct ControlAnims {
    /// Layout key: the component's identity in an [`Animator`].
    pub key: String,
    /// Its animations, offset and size ends in pixels.
    pub graph: AnimGraph,
    pub rest_alpha: f32,
    pub rest_offset: [f32; 2],
    /// Static rect `[x, y, w, h]` and `anchor_to` fraction, which a size animation keeps.
    pub rect: [f64; 4],
    pub anchor: [f64; 2],
    /// Caller creation time of the nearest factory instance, or a clock naming it.
    pub born: Option<f64>,
    pub clock: Option<String>,
    pub disable_fast_forward: bool,
    pub reset_name: Option<String>,
    /// Sprite animations (clip, color, uv) only run on a control with a sprite.
    pub has_sprite: bool,
}

impl ControlAnims {
    /// Whether `other` runs the same animations, whatever their pixel ends.
    pub(crate) fn same_program(&self, other: &ControlAnims) -> bool {
        let graph = (&self.graph, &other.graph);
        graph.0.heads == graph.1.heads
            && graph.0.nodes.len() == graph.1.nodes.len()
            && graph.0.nodes.iter().zip(&graph.1.nodes).all(|(a, b)| {
                a.kind == b.kind
                    && a.duration == b.duration
                    && a.next == b.next
                    && a.play_event == b.play_event
                    && a.reset_event == b.reset_event
            })
    }

    /// The key of this control or the nearest ancestor named `name`.
    pub(crate) fn ancestor_key(&self, name: &str) -> Option<String> {
        let mut end = self.key.len();
        while end > 0 {
            let start = self.key[..end].rfind('/')?;
            let segment = &self.key[start + 1..end];
            let bare = segment.split('[').next().unwrap_or(segment);
            if bare == name {
                return Some(self.key[..end].to_owned());
            }
            end = start;
        }
        None
    }

    pub(crate) fn writes(&self, kind: AnimKind) -> bool {
        self.graph.nodes.iter().any(|node| node.kind == kind)
    }

    pub(crate) fn moves(&self) -> bool {
        self.writes(AnimKind::Offset) || self.writes(AnimKind::Size)
    }
}

/// The animations reaching one draw.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NodeAnim {
    /// The drawn control's own animations (colour, uv, clip).
    pub own: Option<Arc<ControlAnims>>,
    /// Controls whose animated alpha multiplies the draw, itself included.
    pub alpha: Vec<Arc<ControlAnims>>,
    /// Product of the static alphas of the other contributors.
    pub alpha_static: f32,
    /// Offset/size animations moving the draw, and its clip, outermost first.
    pub movers: Vec<Arc<ControlAnims>>,
    pub clip_movers: Vec<Arc<ControlAnims>>,
    /// Static sprite uv origin and size in texture pixels.
    pub uv_rest: [f32; 2],
    pub uv_size_rest: Option<[f32; 2]>,
    /// `clip_direction` when a clip animation crops the draw at paint time.
    pub clip_direction: Option<String>,
    /// The static clip ratio a clip animation replaces.
    pub clip_rest: f32,
}

/// A draw as its animations leave it at one moment.
#[derive(Clone, Debug, PartialEq)]
pub struct Animated {
    pub dest: RectOut,
    pub clip: RectOut,
    pub opacity: f32,
    /// A replacement sprite colour, from a colour animation.
    pub color: Option<[u8; 4]>,
    /// A replacement sprite uv rect, from uv or flip-book animations or a clip crop.
    pub uv: Option<UvRect>,
    /// Removed by `destroy_at_end`.
    pub hidden: bool,
}

impl DrawNode {
    /// This draw under `animator` at `now`; `clocks` hold creation times that
    /// factory clocks name, `textures` sizes flip-book frames.
    pub fn animate(
        &self,
        animator: &mut Animator,
        now: f64,
        clocks: Option<&BTreeMap<String, f64>>,
        textures: Option<&dyn TextureSource>,
    ) -> Animated {
        let mut out = Animated {
            dest: self.dest,
            clip: self.clip,
            opacity: self.alpha,
            color: None,
            uv: None,
            hidden: false,
        };
        let Some(anim) = &self.anim else {
            return out;
        };
        if animator.hides(&self.key) {
            out.hidden = true;
            return out;
        }
        out.opacity = anim
            .alpha
            .iter()
            .fold(anim.alpha_static, |opacity, control| {
                let written = animator.sample(control, now, clocks);
                opacity * written.alpha.unwrap_or(control.rest_alpha)
            });
        if let Some(own) = &anim.own {
            let written = animator.sample(own, now, clocks);
            if let Draw::Sprite { texture, uv, .. } = &self.draw {
                out.color = written.color.map(|color| {
                    color.map(|channel| (channel.clamp(0.0, 1.0) * 255.0).round() as u8)
                });
                let meta = textures.and_then(|source| source.texture(texture));
                let mut written = written;
                if let Some(ms) = written.aseprite_ms {
                    let frames = textures.and_then(|source| source.aseprite_frames(texture));
                    written.uv = Some(aseprite_origin(frames.as_deref().unwrap_or(&[]), ms));
                }
                let uv = sprite_uv(anim, &written, *uv, meta.map(|meta| meta.base_size));
                let (dest, uv) = clip_crop(anim, own, &written, self.dest, uv);
                out.dest = dest;
                out.uv = Some(uv);
            }
        }
        for mover in anim.movers.iter().rev() {
            let written = animator.sample(mover, now, clocks);
            out.dest = moved(mover, &written, out.dest);
        }
        for mover in anim.clip_movers.iter().rev() {
            let written = animator.sample(mover, now, clocks);
            out.clip = moved(mover, &written, out.clip);
        }
        out
    }
}

/// The sprite's uv rect once uv or flip-book animations have written it.
fn sprite_uv(anim: &NodeAnim, written: &Written, uv: UvRect, base: Option<[f64; 2]>) -> UvRect {
    let Some([width, height]) = base.map(|size| size.map(|axis| axis as f32)) else {
        return uv;
    };
    if width <= 0.0 || height <= 0.0 {
        return uv;
    }
    let mut origin = written.uv.unwrap_or(anim.uv_rest);
    let mut size = anim.uv_size_rest.unwrap_or([width, height]);
    if written.uv.is_none() && written.flip.is_none() {
        return uv;
    }
    if let Some(flip) = written.flip {
        let axis = usize::from(flip.vertical);
        let extent = [width, height][axis];
        let step = if flip.count > 0 {
            extent / flip.count as f32
        } else {
            0.0
        };
        origin = [0.0, 0.0];
        origin[axis] = flip.frame * step + FRAME_INSET;
        size[axis] = step + FRAME_SHRINK;
    }
    UvRect {
        u0: origin[0] / width,
        v0: origin[1] / height,
        u1: (origin[0] + size[0]) / width,
        v1: (origin[1] + size[1]) / height,
    }
}

/// Aseprite flipbook timing: the frame whose span holds `ms` into the loop.
fn aseprite_origin(frames: &[crate::sidecar::AsepriteFrame], ms: i64) -> [f32; 2] {
    let total: i64 = frames.iter().map(|frame| frame.duration_ms).sum();
    if total == 0 {
        return [0.0, 0.0];
    }
    let mut left = ms % total;
    for frame in frames {
        if left < frame.duration_ms {
            return [frame.x as f32, frame.y as f32];
        }
        left -= frame.duration_ms;
    }
    [0.0, 0.0]
}

/// A clip animation's crop of the draw, in its static frame.
fn clip_crop(
    anim: &NodeAnim,
    own: &ControlAnims,
    written: &Written,
    dest: RectOut,
    uv: UvRect,
) -> (RectOut, UvRect) {
    let Some(direction) = &anim.clip_direction else {
        return (dest, uv);
    };
    let ratio = written.clip.unwrap_or(anim.clip_rest);
    let [x, y, w, h] = own.rect;
    let visible = crate::sprite::clip_visible(Rect::new(x, y, w, h), ratio, direction);
    let rect = Rect::new(dest.x, dest.y, dest.w, dest.h);
    match crate::sprite::crop(rect, uv, visible) {
        Some((kept, uv)) => (kept.into(), uv),
        None => (
            RectOut {
                w: 0.0,
                h: 0.0,
                ..dest
            },
            uv,
        ),
    }
}

/// `rect` moved by `mover`'s written offset and size.
fn moved(mover: &ControlAnims, written: &Written, rect: RectOut) -> RectOut {
    let mut rect = rect;
    if let Some(size) = written.size {
        let [x, y, w, h] = mover.rect;
        let size = size.map(f64::from);
        let origin = [
            x + mover.anchor[0] * (w - size[0]),
            y + mover.anchor[1] * (h - size[1]),
        ];
        let scale = [
            if w > 0.0 { size[0] / w } else { 0.0 },
            if h > 0.0 { size[1] / h } else { 0.0 },
        ];
        rect = RectOut {
            x: origin[0] + (rect.x - x) * scale[0],
            y: origin[1] + (rect.y - y) * scale[1],
            w: rect.w * scale[0],
            h: rect.h * scale[1],
        };
    }
    if let Some(offset) = written.offset {
        rect.x += f64::from(offset[0] - mover.rest_offset[0]);
        rect.y += f64::from(offset[1] - mover.rest_offset[1]);
    }
    rect
}
