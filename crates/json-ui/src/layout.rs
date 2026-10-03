//! Two-pass layout of a resolved control tree into positioned virtual-pixel rects,
//! solving the same rules as the client's layout variables.
//!
//! Measure (bottom-up) supplies the content extents `%c`/`%cm`/`%sm`/`default` need;
//! place (top-down) resolves each control's size against its parent ([`size`]), then
//! positions it by `anchor_from`/`anchor_to`/`offset`, or as a [`stack`] item or
//! [`grid`] cell. Everything is in the virtual coordinate space of `root_size`; the
//! virtual-to-physical scale is applied downstream by the renderer.
//!
//! Layers are relative: a control draws at its parent's layer plus its own. Each
//! placed control carries a stable key (see [`crate::state`]) so the caller's
//! hover/press/scroll state can drive the engine-owned widget behaviour in
//! [`crate::widgets`].

use serde_json::Value;

use crate::anim::{Inherited, NodeAnim};
use crate::sidecar::TextureMeta;
use crate::state::{LayoutReport, ViewState};
use crate::tree::ResolvedControl;
use crate::widgets;

mod grid;
mod measure;
mod place;
mod refresh;
mod scroll;
mod size;
mod stack;
mod style;

pub(crate) use grid::TEMPLATE_KEY as GRID_TEMPLATE_KEY;
pub use measure::MeasureCache;

pub(crate) use place::draggable_axes;
use place::{control_anims, place_by_anchor};
use scroll::{Adjusted, ScrollFrame};

/// A virtual-pixel rectangle, top-left origin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }

    /// The overlap of two rects, clamped so width/height never go negative.
    pub fn intersect(self, other: Rect) -> Rect {
        let x0 = self.x.max(other.x);
        let y0 = self.y.max(other.y);
        let x1 = (self.x + self.w).min(other.x + other.w);
        let y1 = (self.y + self.h).min(other.y + other.h);
        Rect::new(x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0))
    }
}

/// Natural size of a label's text, in virtual pixels. The real font binds later; a
/// caller without one can return zero, which lays a label out as an empty extent.
pub trait TextMeasure {
    fn extent(&self, text: &str) -> [f64; 2];

    /// The extent when wrapped at `max_width`; a measurer without wrapping keeps
    /// the single-line extent.
    fn wrapped(&self, text: &str, max_width: f64) -> [f64; 2] {
        let _ = max_width;
        self.extent(text)
    }

    /// Measures a label in a named font; default-only backends keep their label metrics.
    fn named_label(
        &self,
        text: &str,
        _font: &str,
        width: Option<f64>,
        shape: crate::label::LabelShape,
    ) -> [f64; 2] {
        self.label(text, width, shape)
    }

    /// A localizing label's text as it will draw; measurers without a language
    /// table measure it as written.
    fn localize<'a>(&self, text: &'a str) -> std::borrow::Cow<'a, str> {
        std::borrow::Cow::Borrowed(text)
    }

    /// A label's extent in `shape`, wrapped at `max_width` when known; the
    /// default scales the unscaled measure and ignores line padding.
    fn label(
        &self,
        text: &str,
        max_width: Option<f64>,
        shape: crate::label::LabelShape,
    ) -> [f64; 2] {
        let [w, h] = match max_width {
            Some(width) if width > 0.0 => self.wrapped(text, width / shape.scale),
            _ => self.extent(text),
        };
        [w * shape.scale, h * shape.scale]
    }
}

/// Resolves a `texture` path to its sidecar metadata (base size, nine-slice). The
/// atlas is bound later; only the metadata is needed to size and slice a sprite.
pub trait TextureSource {
    fn texture(&self, path: &str) -> Option<TextureMeta>;

    /// An aseprite sheet's frames, for `aseprite_flip_book`.
    fn aseprite_frames(&self, _path: &str) -> Option<Vec<crate::sidecar::AsepriteFrame>> {
        None
    }
}

/// The measurement backends layout and emit share.
pub struct LayoutEnv<'a> {
    pub text: &'a dyn TextMeasure,
    pub textures: &'a dyn TextureSource,
}

/// A placed control: the borrowed definition plus its resolved geometry and the
/// clip region it draws within.
#[derive(Clone, Debug)]
pub struct LaidOut<'a> {
    pub control: &'a ResolvedControl,
    /// Stable address for [`ViewState`] lookups.
    pub key: String,
    pub rect: Rect,
    pub clip: Rect,
    /// Absolute draw layer (the parent's plus this control's own).
    pub layer: i32,
    pub alpha: f32,
    /// Animations reaching this control's draws, evaluated at paint time.
    pub anim: Option<std::sync::Arc<NodeAnim>>,
    pub visible: bool,
    /// False when this control or an ancestor is disabled (locked styling, no input).
    pub enabled: bool,
    /// Fraction clipped off a progress image by its widget (`clip_direction`).
    pub clip_ratio: Option<f32>,
    pub children: Vec<LaidOut<'a>>,
    /// Bound-tree state masks reused by the gated emit pass.
    pub(crate) state_targets: Option<measure::Targets>,
}

#[derive(Clone, Copy, PartialEq)]
enum Axis {
    X,
    Y,
}

/// Lay `root` out inside a virtual screen of `root_size`, positioning it as the lone
/// child of that screen.
pub fn layout<'a>(root: &'a ResolvedControl, root_size: [f64; 2], env: &LayoutEnv) -> LaidOut<'a> {
    layout_with(root, root_size, env, &ViewState::default()).0
}

/// [`layout`] driven by live interaction state, also reporting scroll extents.
pub fn layout_with<'a>(
    root: &'a ResolvedControl,
    root_size: [f64; 2],
    env: &LayoutEnv,
    state: &ViewState,
) -> (LaidOut<'a>, LayoutReport) {
    measure::reset();
    lay_out(root, root_size, env, state, false)
}

/// Lay out a stable bound tree using retained measurements, with ordinary visibility.
pub(crate) fn layout_cached<'a>(
    root: &'a ResolvedControl,
    root_size: [f64; 2],
    env: &LayoutEnv,
    state: &ViewState,
    cache: &mut MeasureCache,
) -> (LaidOut<'a>, LayoutReport) {
    cache.enter(root);
    let laid = lay_out(root, root_size, env, state, false);
    cache.leave();
    laid
}

/// [`layout_with`] over `cache`'s measurements that omits hidden controls'
/// subtrees and scroll content wholly outside its viewport, so a long list costs
/// only what it shows.
pub(crate) fn layout_culled<'a>(
    root: &'a ResolvedControl,
    root_size: [f64; 2],
    env: &LayoutEnv,
    state: &ViewState,
    cache: &mut MeasureCache,
) -> (LaidOut<'a>, LayoutReport) {
    cache.enter(root);
    let laid = lay_out(root, root_size, env, state, true);
    cache.leave();
    laid
}

/// A culling layout keeps the memos its caller entered.
fn lay_out<'a>(
    root: &'a ResolvedControl,
    root_size: [f64; 2],
    env: &LayoutEnv,
    state: &ViewState,
    cull: bool,
) -> (LaidOut<'a>, LayoutReport) {
    let screen = Rect::new(0.0, 0.0, root_size[0], root_size[1]);
    let own = size::resolve_size(root, [Some(screen.w), Some(screen.h)], [0.0; 2], env);
    let rect = place_by_anchor(root, screen, own, [0.0; 2], env);
    let mut ctx = PlaceCtx {
        env,
        state,
        cull,
        report: LayoutReport::default(),
        scrolls: Vec::new(),
        sliders: Vec::new(),
        ancestors: Vec::new(),
        overrides: Vec::new(),
        disabled: 0,
        hidden_names: Vec::new(),
        screen,
    };
    let key = child_key("", root, 0);
    let laid = place_subtree(
        root,
        key,
        rect,
        screen,
        (0, true, false, true),
        &Inherited::default(),
        &mut ctx,
    );
    (laid, ctx.report)
}

struct PlaceCtx<'tree, 'e, 'x> {
    env: &'e LayoutEnv<'x>,
    state: &'e ViewState,
    /// Skip placing scroll content wholly outside its viewport.
    cull: bool,
    report: LayoutReport,
    scrolls: Vec<ScrollFrame>,
    /// Enclosing sliders: fraction, box and progress names, rect, and axis.
    sliders: Vec<widgets::SliderFrame>,
    /// Enclosing controls' names, rects, and child clips, for `dropdown_area`.
    ancestors: Vec<(&'tree str, Rect, Rect)>,
    /// State controls enclosing stateful controls show or hide: target, shown, state mask.
    overrides: Vec<(usize, bool, u8)>,
    /// Enclosing disabled controls; their descendants are locked.
    disabled: usize,
    /// Descendant names an enclosing edit box hides (its placeholder).
    hidden_names: Vec<String>,
    /// What a control that opts out of clipping draws within.
    screen: Rect,
}

/// `parent/name`, with `[index]` on factory instances and `~n` on the nth sibling sharing
/// both, so anonymous array entries keep their own hover and press state.
pub(crate) fn child_key(parent: &str, control: &ResolvedControl, repeat: usize) -> String {
    instance_key(parent, &control.name, collection_index(control), repeat)
}

/// The shared identity of an instance, including patched names and collection indices.
pub(crate) fn instance_key(parent: &str, name: &str, index: Option<u64>, repeat: usize) -> String {
    let mut key = String::with_capacity(parent.len() + name.len() + 6);
    key.push_str(parent);
    key.push('/');
    key.push_str(name);
    if let Some(index) = index {
        key.push('[');
        key.push_str(&index.to_string());
        key.push(']');
    }
    if repeat > 0 {
        key.push('~');
        key.push_str(&(repeat + 1).to_string());
    }
    key
}

fn collection_index(control: &ResolvedControl) -> Option<u64> {
    control
        .properties
        .get("collection_index")
        .and_then(Value::as_u64)
}

/// Counts siblings by name and collection index for [`child_key`]'s repeat.
#[derive(Default)]
pub(crate) struct SiblingKeys(std::collections::HashMap<(String, Option<u64>), usize>);

impl SiblingKeys {
    /// Earlier siblings sharing `name` and `index`.
    pub(crate) fn repeat(&mut self, name: &str, index: Option<u64>) -> usize {
        let count = self.0.entry((name.to_owned(), index)).or_default();
        *count += 1;
        *count - 1
    }

    pub(crate) fn of(&mut self, control: &ResolvedControl) -> usize {
        self.repeat(&control.name, collection_index(control))
    }
}

#[allow(clippy::too_many_arguments)]
fn place_subtree<'a>(
    control: &'a ResolvedControl,
    key: String,
    rect: Rect,
    parent_clip: Rect,
    (parent_layer, shown, packed, parent_allows): (i32, bool, bool, bool),
    inherited: &Inherited,
    ctx: &mut PlaceCtx<'a, '_, '_>,
) -> LaidOut<'a> {
    // A state control a stateful ancestor shows or hides overrides its own `visible`.
    let forced = ctx
        .overrides
        .iter()
        .rev()
        .find(|(target, _, _)| *target == std::ptr::from_ref(control).addr())
        .map(|(_, shown, mask)| (*shown, *mask));
    let style = measure::style(control);
    let own_visible = forced.map_or(
        style.visible && !measure::suppressed(control),
        |(shown, _)| shown,
    );
    let clips = style.clips;
    let child_clip = if clips {
        inset_clip(rect, style.clip_offset, parent_clip)
    } else {
        parent_clip
    };
    // `allow_clipping` defaults to the parent's; opting out frees only the
    // control's own drawing, not its children's.
    let allows = style.allows.unwrap_or(parent_allows);
    let own_clip = match (allows, clips) {
        (false, _) => ctx.screen,
        (true, true) => child_clip,
        (true, false) => parent_clip,
    };
    let enabled = ctx.disabled == 0 && style.enabled;
    if !enabled {
        ctx.disabled += 1;
    }
    let parent_rect = ctx
        .ancestors
        .last()
        .map_or(parent_clip, |(_, parent, _)| *parent);
    let own_anims = control_anims(control, &key, rect, parent_rect, inherited, packed, ctx.env);
    let (own_alpha, anim, inherit) =
        inherited.apply(control, style.alpha, own_anims, clips, |node| {
            place::sprite_rest(control, node)
        });
    let absolute_layer = parent_layer.saturating_add(style.layer);
    let mut scroll = ScrollFrame::open(control, &key, rect, ctx.state, ctx.env);
    // A bar panel hidden or shown again frees or takes back its space: solve again.
    if let Some(frame) = &scroll
        && let Some(panel) = frame.panel_address()
        && measure::suppress(panel, frame.panel_hidden)
    {
        scroll = ScrollFrame::open(control, &key, rect, ctx.state, ctx.env);
    }
    let opened_scroll = scroll.is_some();
    if let Some(frame) = scroll {
        ctx.scrolls.push(frame);
    }
    let slider = widgets::SliderFrame::open(control, rect);
    let opened_slider = slider.is_some();
    if let Some(entry) = slider {
        ctx.sliders.push(entry);
    }
    let overrides_len = ctx.overrides.len();
    let bits = widgets::state_index(ctx.state, &key);
    let state_targets = measure::state_targets(control, !enabled);
    ctx.overrides.extend(
        state_targets
            .iter()
            .flat_map(|targets| targets.iter())
            .map(|&(target, mask)| (target, mask & (1 << bits) != 0, mask)),
    );
    let placeholder = widgets::hidden_placeholder(control);
    if let Some(name) = placeholder {
        ctx.hidden_names.push(name.to_owned());
    }
    let dropdown = widgets::dropdown_area(control);
    ctx.ancestors.push((&control.name, rect, child_clip));
    // A culling layout leaves a hidden control's subtree unplaced: nothing in it draws.
    let placed = if ctx.cull && !own_visible && forced.is_none_or(|(_, mask)| mask == 0) {
        Vec::new()
    } else {
        let indexed_clip = (ctx.cull
            && grid::is_grid(control)
            && allows
            && !style.unclipped_descendant
            && control.children.iter().all(|child| {
                let style = measure::style(child);
                child.children.is_empty() || style.clips
            })
            && dropdown.is_none()
            && ctx.sliders.is_empty()
            && ctx
                .scrolls
                .last()
                .is_some_and(|frame| frame.metrics.is_some())
            && !ctx
                .scrolls
                .iter()
                .any(|frame| frame.adjusts_children(control)))
        .then_some(child_clip);
        measure::placed_children(control, rect, ctx.env, indexed_clip)
    };
    let placed_rects: Vec<(&str, Rect)> = match &dropdown {
        Some(_) => placed
            .iter()
            .map(|(child, rect)| (child.name.as_str(), *rect))
            .collect(),
        None => Vec::new(),
    };
    let priority = stack::hidden_by_priority(control, rect, ctx.env);
    let packs = stack::orientation(control).is_some() || grid::is_grid(control);
    let mut children = Vec::with_capacity(placed.len());
    let repeats = if control.children.len() > 1 {
        let mut siblings = SiblingKeys::default();
        control
            .children
            .iter()
            .map(|child| siblings.of(child))
            .collect()
    } else {
        Vec::new()
    };
    for (child, mut child_rect) in placed {
        let child_shown = !ctx.hidden_names.contains(&child.name)
            && !priority
                .get(measure::child_index(control, child))
                .copied()
                .unwrap_or(false);
        let mut clip_for_child = child_clip;
        // A dropdown's content drops from the dropdown, kept inside its named area.
        if let Some((drop, area, content)) = &dropdown
            && *content == child.name
            && let Some((_, area_rect, area_clip)) = ctx
                .ancestors
                .iter()
                .rev()
                .find(|(name, _, _)| *name == area.as_str())
            && let Some((_, drop_rect)) =
                placed_rects.iter().find(|(name, _)| *name == drop.as_str())
        {
            child_rect.y = widgets::dropdown_content_top(*drop_rect, *area_rect, child_rect.h);
            clip_for_child = *area_clip;
        }
        let mut child_shown = child_shown;
        let mut box_fade = None;
        for frame in ctx.scrolls.iter().rev() {
            match frame.adjust(child, child_rect) {
                Adjusted::Kept => continue,
                Adjusted::Moved(moved) => child_rect = moved,
                Adjusted::Box(moved, fade) => (child_rect, box_fade) = (moved, fade),
                Adjusted::Hidden => child_shown = false,
            }
            break;
        }
        let repeat = repeats
            .get(measure::child_index(control, child))
            .copied()
            .unwrap_or(0);
        let next_key = child_key(&key, child, repeat);
        // Stack items and grid cells have no offset delta term.
        if !packs {
            if place::follows_pointer(child) {
                ctx.report.tracks_pointer = true;
            }
            let dragged = ctx.state.drags.get(&next_key).copied();
            if let Some(moved) =
                place::offset_delta(child, child_rect, rect, ctx.state.pointer, dragged)
            {
                child_rect = moved;
            }
        }
        // The named slider box travels the slider however deep it sits.
        if let Some(frame) = ctx.sliders.last()
            && frame.names[0].as_deref() == Some(child.name.as_str())
        {
            child_rect = frame.place_box(child_rect);
        }
        // Scroll content wholly outside its viewport neither draws nor takes input.
        if ctx.cull
            && ctx
                .scrolls
                .last()
                .is_some_and(|frame| frame.metrics.is_some())
            && allows
            && measure::style(child).allows != Some(false)
            && !measure::style(child).unclipped_descendant
            && (child.children.is_empty() || measure::style(child).clips)
            && disjoint(child_rect, clip_for_child)
        {
            continue;
        }
        let mut laid = place_subtree(
            child,
            next_key,
            child_rect,
            clip_for_child,
            (absolute_layer, child_shown, packs, allows),
            &inherit,
            ctx,
        );
        // A fading touch box dims its children, as vanilla writes their alpha.
        if let Some(fade) = box_fade {
            for child in &mut laid.children {
                child.alpha *= fade;
            }
        }
        children.push(laid);
    }
    ctx.ancestors.pop();
    ctx.overrides.truncate(overrides_len);
    if placeholder.is_some() {
        ctx.hidden_names.pop();
    }
    if !enabled {
        ctx.disabled -= 1;
    }
    if opened_slider {
        ctx.sliders.pop();
    }
    if opened_scroll
        && let Some(frame) = ctx.scrolls.pop()
        && let Some(metrics) = frame.metrics
    {
        ctx.report.scrolls.insert(frame.key, metrics);
    }
    if let Some(event) = control
        .properties
        .get("clip_state_change_event")
        .and_then(Value::as_str)
        .filter(|_| allows)
    {
        ctx.report
            .clip_states
            .insert(key.clone(), (event.to_owned(), clipped_out(rect, own_clip)));
    }
    LaidOut {
        control,
        clip_ratio: progress_clip(control, &ctx.sliders),
        key,
        rect,
        clip: own_clip,
        layer: absolute_layer,
        alpha: own_alpha,
        anim,
        visible: shown && own_visible,
        enabled,
        children,
        state_targets,
    }
}

/// The clip a `clips_children` control gives its children: its rect inset by
/// `clip_offset` on every side, within `parent`, never inverted.
fn inset_clip(rect: Rect, offset: [f64; 2], parent: Rect) -> Rect {
    let x0 = (rect.x + offset[0]).max(parent.x);
    let y0 = (rect.y + offset[1]).max(parent.y);
    let x1 = (rect.x + rect.w - offset[0])
        .min(parent.x + parent.w)
        .max(x0);
    let y1 = (rect.y + rect.h - offset[1])
        .min(parent.y + parent.h)
        .max(y0);
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}

/// Whether `rect` lies wholly outside `clip`, compared in whole pixels; touching
/// edges and a zero-area clip count as visible.
fn clipped_out(rect: Rect, clip: Rect) -> bool {
    let (w, h) = (clip.w.round(), clip.h.round());
    w != 0.0
        && h != 0.0
        && (rect.w.round() + rect.x.round() < clip.x.round()
            || rect.h.round() + rect.y.round() < clip.y.round()
            || w + clip.x.round() < rect.x.round()
            || h + clip.y.round() < rect.y.round())
}

/// True when `rect` and `clip` share no area.
fn disjoint(rect: Rect, clip: Rect) -> bool {
    rect.x >= clip.x + clip.w
        || rect.y >= clip.y + clip.h
        || rect.x + rect.w <= clip.x
        || rect.y + rect.h <= clip.y
}

/// A bound `clip_ratio`, or a slider progress image revealing its fraction.
fn progress_clip(control: &ResolvedControl, sliders: &[widgets::SliderFrame]) -> Option<f32> {
    if let Some(widgets::SliderFrame {
        fraction, names, ..
    }) = sliders.last()
        && (names[1].as_deref() == Some(control.name.as_str())
            || names[2].as_deref() == Some(control.name.as_str()))
    {
        return Some((1.0 - fraction) as f32);
    }
    widgets::bound_number(control, "clip_ratio").map(|ratio| ratio.clamp(0.0, 1.0) as f32)
}

/// Resolve the rects of `parent`'s direct children within `parent_rect`.
fn layout_children<'a>(
    parent: &'a ResolvedControl,
    parent_rect: Rect,
    env: &LayoutEnv,
) -> Vec<(&'a ResolvedControl, Rect)> {
    if grid::is_grid(parent) {
        return grid::grid_children(parent, parent_rect, env);
    }
    if stack::orientation(parent).is_some() {
        return stack::stack_children(parent, parent_rect, env);
    }
    let extent = [Some(parent_rect.w), Some(parent_rect.h)];
    let sizes = measure::sizes(parent, extent, env);
    let siblings = measure::sibling_maxima(parent, &sizes);
    parent
        .children
        .iter()
        .zip(sizes.iter())
        .map(|(child, own)| {
            (
                child,
                place_by_anchor(child, parent_rect, *own, siblings, env),
            )
        })
        .collect()
}

// --- property readers -------------------------------------------------------

/// The axis a stack panel packs along; `none` and other controls have none.
fn stack_axis(control: &ResolvedControl) -> Option<Axis> {
    stack::main_axis(control)
}

/// Own visibility, less a scroll bar panel hidden while its content fits.
fn visible(control: &ResolvedControl) -> bool {
    measure::style(control).visible && !measure::suppressed(control)
}

// --- axis helpers -----------------------------------------------------------

fn other(axis: Axis) -> Axis {
    match axis {
        Axis::X => Axis::Y,
        Axis::Y => Axis::X,
    }
}

fn axis_index(axis: Axis) -> usize {
    match axis {
        Axis::X => 0,
        Axis::Y => 1,
    }
}
