//! Scroll views as the vanilla client lays them out: the named
//! viewport, content, track, box and bar panel are found breadth-first from the
//! view (tree order does not matter), the content shifts by the clamped offset
//! snapped to 1/8 px, the box takes `clamp(viewport / content, 0.1, 1)` of the
//! track, and the bar panel hides while the content fits. A touch motion may
//! carry the offset a quarter viewport past either end, and a touch-mode box
//! fades out after its last touch.

use std::cell::RefCell;

use serde_json::Value;

use super::{LayoutEnv, Rect, ResolvedControl, measure, place};
use crate::state::{ScrollMetrics, ViewState};
use crate::widgets::{self, Draggable, OVERSCROLL};

/// `#scrollbar_hit_bottom` latches within this many pixels of the end.
const HIT_BOTTOM_EPSILON: f64 = 0.1;

/// The named roles, in `ScrollRoles` order.
const ROLE_KEYS: [&str; 5] = [
    "scroll_view_port",
    "scroll_content",
    "scrollbar_track",
    "scrollbar_box",
    "scroll_box_and_track_panel",
];
const VIEWPORT: usize = 0;
const CONTENT: usize = 1;
const TRACK: usize = 2;
const BOX: usize = 3;
const PANEL: usize = 4;

/// Child-index paths from the view to each named role.
type Roles = [Option<Vec<usize>>; 5];

/// Role paths by view address, kept with a tree's other measurements.
pub(super) type RoleMemo = measure::Memo<usize, Roles>;

thread_local! {
    static ROLES: RefCell<RoleMemo> = RefCell::new(RoleMemo::default());
}

pub(super) fn swap(memo: &mut RoleMemo) {
    ROLES.with(|live| std::mem::swap(&mut *live.borrow_mut(), memo));
}

/// The first control named `name` breadth-first from `root`, itself included.
fn breadth_first(root: &ResolvedControl, name: &str) -> Option<Vec<usize>> {
    let mut queue = std::collections::VecDeque::from([(root, Vec::new())]);
    while let Some((node, path)) = queue.pop_front() {
        if node.name == name {
            return Some(path);
        }
        for (index, child) in node.children.iter().enumerate() {
            let mut next = path.clone();
            next.push(index);
            queue.push_back((child, next));
        }
    }
    None
}

fn roles(view: &ResolvedControl) -> Roles {
    let address = std::ptr::from_ref(view).addr();
    if let Some(found) = ROLES.with(|memo| memo.borrow().get(&address).cloned()) {
        return found;
    }
    let found: Roles = ROLE_KEYS.map(|key| {
        let name = view
            .properties
            .get(key)
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())?;
        breadth_first(view, name)
    });
    ROLES.with(|memo| memo.borrow_mut().insert(address, found.clone()));
    found
}

/// Forget only the moved root; descendant allocations remain stable.
pub(super) fn forget(address: usize) {
    ROLES.with(|memo| {
        memo.borrow_mut().remove(&address);
    });
}

/// Forget the role paths of every view (a new tree).
pub(super) fn reset() {
    ROLES.with(|memo| memo.borrow_mut().clear());
}

/// The control at `path` under `view` and its unscrolled rect, with its parent's.
fn locate<'a>(
    view: &'a ResolvedControl,
    rect: Rect,
    path: &[usize],
    env: &LayoutEnv,
) -> Option<(&'a ResolvedControl, Rect, Rect)> {
    let mut node = view;
    let mut at = rect;
    let mut parent = rect;
    for &index in path {
        let child = node.children.get(index)?;
        let placed = measure::placed_children(node, at, env, None);
        let (_, child_rect) = placed
            .into_iter()
            .find(|(placed, _)| std::ptr::eq(*placed, child))?;
        parent = at;
        at = child_rect;
        node = child;
    }
    Some((node, at, parent))
}

fn address(control: &ResolvedControl) -> usize {
    std::ptr::from_ref(control).addr()
}

/// Whether `control` drags horizontally (`draggable`).
fn horizontal(control: &ResolvedControl) -> bool {
    control.properties.get("draggable").and_then(Value::as_str) == Some("horizontal")
}

/// The scrollbar box's axis from its `draggable`, or `None` when it has none.
fn box_axis(control: &ResolvedControl) -> Option<usize> {
    match control.properties.get("draggable").and_then(Value::as_str) {
        Some("horizontal") => Some(0),
        Some("vertical") => Some(1),
        _ => None,
    }
}

/// A live scroll view: its solved geometry, applied as its descendants are placed.
pub(crate) struct ScrollFrame {
    pub key: String,
    content: Option<usize>,
    bar_box: Option<usize>,
    panel: Option<usize>,
    /// The content's shift from its laid-out rect.
    delta: [f64; 2],
    box_rect: Option<Rect>,
    /// A touch-mode box's children's alpha while it fades; `Some(0)` hides it.
    box_fade: Option<f32>,
    /// Whether the bar panel hides because the content fits.
    pub panel_hidden: bool,
    pub metrics: Option<ScrollMetrics>,
}

impl ScrollFrame {
    /// Whether a direct child still needs a scroll-role transform before culling.
    pub(super) fn adjusts_children(&self, parent: &ResolvedControl) -> bool {
        let start = parent.children.as_ptr().addr();
        let end = start + std::mem::size_of_val(parent.children.as_slice());
        [self.content, self.bar_box, self.panel]
            .into_iter()
            .flatten()
            .any(|address| address >= start && address < end)
    }

    /// Solve `view`'s scroll geometry within `rect`.
    pub fn open(
        view: &ResolvedControl,
        key: &str,
        rect: Rect,
        state: &ViewState,
        env: &LayoutEnv,
    ) -> Option<Self> {
        if view.control_type.as_deref() != Some("scroll_view") {
            return None;
        }
        let paths = roles(view);
        let find = |role: usize| {
            paths[role]
                .as_ref()
                .and_then(|path| locate(view, rect, path, env))
        };
        let content = find(CONTENT);
        let viewport = find(VIEWPORT);
        let track = find(TRACK);
        let bar_box = find(BOX);
        let panel = find(PANEL);
        let mut frame = Self {
            key: key.to_owned(),
            content: content.map(|(control, _, _)| address(control)),
            bar_box: bar_box.map(|(control, _, _)| address(control)),
            panel: panel.map(|(control, _, _)| address(control)),
            delta: [0.0; 2],
            box_rect: None,
            box_fade: None,
            panel_hidden: false,
            metrics: None,
        };
        // Vanilla scrolls only with all four references resolved, along
        // the box's `draggable` axis.
        let (
            Some((content, content_rect, _)),
            Some((_, viewport_rect, _)),
            Some(_),
            Some((box_control, _, _)),
        ) = (content, viewport, track, bar_box)
        else {
            return Some(frame);
        };
        let axis = usize::from(!horizontal(box_control));
        let extent = |rect: Rect| [rect.w, rect.h];
        let content_size = extent(content_rect);
        let viewport_size = extent(viewport_rect);
        let ext = [
            (content_size[0] - viewport_size[0]).max(0.0),
            (content_size[1] - viewport_size[1]).max(0.0),
        ];
        let max = ext[axis];
        let retained = state.scroll_state.get(key);
        let jump_to_end = widgets::bound_bool(view, "jump_to_bottom_on_update") == Some(true);
        // A caller that never settles keeps its own offset once it sets one.
        let known = retained
            .and_then(|retained| retained.extent)
            .or_else(|| state.scroll.contains_key(key).then_some(f64::NAN));
        let grew = jump_to_end && known.is_none_or(|known| !known.is_nan() && known != max);
        let force = widgets::bound_bool(view, "#force_scroll_to_end") == Some(true);
        let moving = retained.is_some_and(|retained| retained.motion.is_some());
        let slack = if moving {
            viewport_size[axis] * OVERSCROLL
        } else {
            0.0
        };
        let position = if grew || force {
            max
        } else {
            state.scroll_offset(key).clamp(-slack, max + slack)
        };
        let shown = (position * 8.0).trunc() * 0.125;
        let from = place::anchor_from(content);
        let mut delta = [0.0; 2];
        delta[axis] = -shown;
        if from[0] == 1.0 {
            delta[0] += ext[0];
        }
        if from[1] == 1.0 {
            delta[1] += ext[1];
        }
        frame.delta = delta;
        let fits = content_size[axis] <= 0.0 || viewport_size[axis] / content_size[axis] >= 1.0;
        let always = widgets::bound_bool(view, "scrollbar_always_visible") == Some(true);
        let thumb_axis = bar_box.and_then(|(control, _, _)| box_axis(control));
        frame.panel_hidden = thumb_axis.is_some() && panel.is_some() && fits && !always;
        let touch_mode = widgets::bound_bool(view, "touch_mode") == Some(true);
        frame.box_fade = retained
            .and_then(|retained| retained.bar_fade)
            .filter(|_| touch_mode);
        // The bottom latches: content that fits, the end reached, or the overflow passed.
        let overflow_y = content_size[1] - viewport_size[1];
        let hit_bottom = retained.is_some_and(|retained| retained.hit_bottom)
            || (frame.panel_hidden || (thumb_axis.is_some() && panel.is_some() && fits))
            || (axis == 1 && (shown - max).abs() < HIT_BOTTOM_EPSILON)
            || (shown != 0.0 && overflow_y <= shown);
        let bar_visible = match frame.box_fade {
            Some(0.0) => Some(false),
            Some(_) => Some(true),
            None => panel.is_some().then_some(!frame.panel_hidden),
        };
        let name = |key: &str| {
            view.properties
                .get(key)
                .and_then(Value::as_str)
                .map(str::to_owned)
        };
        let rect_of = |rect: Rect| [rect.x, rect.y, rect.w, rect.h];
        let mut metrics = ScrollMetrics {
            offset: position,
            content: content_size[axis],
            viewport: viewport_size[axis],
            viewport_top: [viewport_rect.x, viewport_rect.y][axis],
            viewport_rect: Some(rect_of(viewport_rect)),
            content_rect: Some(rect_of(content_rect)),
            track: track.map(|(_, rect, _)| rect_of(rect)),
            thumb: None,
            speed: widgets::bound_number(view, "scroll_speed").unwrap_or(1.0),
            horizontal: axis == 0,
            box_drag: bar_box.map_or(Draggable::NotDraggable, |(control, _, _)| {
                Draggable::of(control)
            }),
            gesture: widgets::bound_bool(view, "#gesture_control_enabled")
                .or_else(|| widgets::bound_bool(view, "gesture_control_enabled"))
                .unwrap_or(false),
            always_handle_scrolling: widgets::bound_bool(view, "always_handle_scrolling")
                == Some(true),
            touch_mode,
            allow_scroll_when_fits: widgets::bound_bool(
                view,
                "allow_scroll_even_when_content_fits",
            )
            .unwrap_or(true),
            jump_to_end,
            track_button: name("scrollbar_track_button"),
            touch_button: name("scrollbar_touch_button"),
            bar_visible,
            hit_bottom,
            scrolled_to_end: max <= position,
        };
        // The box travels the track along the scroll axis; a one-axis box under a
        // named panel also takes the visible fraction of the track.
        if let (Some((control, box_rect, box_parent)), Some((_, track_rect, _))) = (bar_box, track)
            && frame.box_fade != Some(0.0)
            && !frame.panel_hidden
        {
            let track_size = extent(track_rect);
            let mut size = extent(box_rect);
            if let Some(thumb_axis) = thumb_axis.filter(|_| panel.is_some()) {
                let ratio = if content_size[thumb_axis] > 0.0 {
                    (viewport_size[thumb_axis] / content_size[thumb_axis]).clamp(0.1, 1.0)
                } else {
                    1.0
                };
                size[thumb_axis] = (ratio * track_size[thumb_axis]).ceil();
            }
            let base = place::place_by_anchor(control, box_parent, size, [0.0; 2], env);
            let travel = track_size[axis] - size[axis];
            let span = ext[axis];
            let fraction = if span > 0.5 { shown / span } else { 1.0 };
            let mut at = [base.x, base.y];
            at[axis] += travel * fraction;
            let placed = Rect::new(at[0], at[1], size[0], size[1]);
            metrics.thumb = Some([placed.x, placed.y, placed.w, placed.h]);
            frame.box_rect = Some(placed);
        }
        frame.metrics = Some(metrics);
        Some(frame)
    }

    /// The control this view shifts, sizes, fades or hides, and how.
    pub fn adjust(&self, child: &ResolvedControl, rect: Rect) -> Adjusted {
        let at = address(child);
        if self.content == Some(at) && self.metrics.is_some() {
            return Adjusted::Moved(Rect::new(
                rect.x + self.delta[0],
                rect.y + self.delta[1],
                rect.w,
                rect.h,
            ));
        }
        if self.bar_box == Some(at) {
            if self.box_fade == Some(0.0) {
                return Adjusted::Hidden;
            }
            if let Some(placed) = self.box_rect {
                return Adjusted::Box(placed, self.box_fade);
            }
        }
        Adjusted::Kept
    }

    /// The bar panel's address, when the view names one.
    pub fn panel_address(&self) -> Option<usize> {
        self.panel
    }
}

pub(crate) enum Adjusted {
    Kept,
    Moved(Rect),
    /// The scrollbar box at its rect, its children faded by a touch-mode fade.
    Box(Rect, Option<f32>),
    Hidden,
}
