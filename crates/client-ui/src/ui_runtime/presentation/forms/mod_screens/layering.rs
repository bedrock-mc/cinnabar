//! Where a player mod's overlay may draw beside a container screen, and the node reordering
//! that keeps vanilla's held stack and tooltips above it.

use std::{collections::BTreeMap, ops::Range};

use server_experience::screen::{Rect, ScreenLayout};
use ui::{UiNode, UiNodeId, UiVisual};

use crate::ui_runtime::forms::EngineFrame;

/// A control covering at least this share of the root on both axes is a full-screen one (a
/// dismiss area), not part of the vanilla panels.
const FULL_SCREEN_SHARE: f64 = 0.9;

/// The GUI rect: the union of the vanilla screen's `root_panel` and every control it laid out
/// (an open recipe book's included), but full-screen ones. `None` when nothing is laid out.
pub(super) fn gui_rect(
    panel: Option<[f64; 4]>,
    controls: impl Iterator<Item = [f64; 4]>,
    root: [f64; 2],
) -> Option<Rect> {
    let full_screen = |[_, _, w, h]: &[f64; 4]| {
        *w >= root[0] * FULL_SCREEN_SHARE && *h >= root[1] * FULL_SCREEN_SHARE
    };
    let [left, top, right, bottom] = panel
        .into_iter()
        .chain(controls.filter(|rect| !full_screen(rect)))
        .filter(|[_, _, w, h]| *w > 0.0 && *h > 0.0)
        .map(|[x, y, w, h]| [x, y, x + w, y + h])
        .reduce(|a, b| {
            [
                a[0].min(b[0]),
                a[1].min(b[1]),
                a[2].max(b[2]),
                a[3].max(b[3]),
            ]
        })?;
    Some(Rect {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    })
}

/// The bounds of the visible nodes in `nodes`, in GUI units at `scale` logical pixels per unit,
/// leaving out full-screen ones (a dimmed background) of a `content`-sized root.
pub(super) fn drawn_bounds(nodes: &[UiNode], content: [f32; 2], scale: f32) -> Option<Rect> {
    let by_id: BTreeMap<UiNodeId, &UiNode> = nodes.iter().map(|node| (node.id(), node)).collect();
    let full_screen = |width: f32, height: f32| {
        f64::from(width) >= f64::from(content[0]) * FULL_SCREEN_SHARE
            && f64::from(height) >= f64::from(content[1]) * FULL_SCREEN_SHARE
    };
    let [left, top, right, bottom] = nodes
        .iter()
        .filter(|node| !matches!(node.visual(), UiVisual::None))
        .filter_map(|node| clipped_bounds(node, &by_id, content))
        .filter(|[left, top, right, bottom]| !full_screen(right - left, bottom - top))
        .reduce(|a, b| {
            [
                a[0].min(b[0]),
                a[1].min(b[1]),
                a[2].max(b[2]),
                a[3].max(b[3]),
            ]
        })?;
    let gui = |value: f32| f64::from(value / scale);
    Some(Rect {
        x: gui(left),
        y: gui(top),
        width: gui(right - left),
        height: gui(bottom - top),
    })
}

/// Translates a node through its ancestors and intersects every ancestor clip and the viewport.
/// Missing parents, cycles, and completely clipped nodes have no visible bounds.
fn clipped_bounds(
    node: &UiNode,
    nodes: &BTreeMap<UiNodeId, &UiNode>,
    content: [f32; 2],
) -> Option<[f32; 4]> {
    let bounds = node.bounds();
    let mut rect = [
        bounds.min().x(),
        bounds.min().y(),
        bounds.max().x(),
        bounds.max().y(),
    ];
    let mut parent = node.parent();
    for _ in 0..=nodes.len() {
        let Some(id) = parent else {
            for axis in 0..2 {
                rect[axis] = rect[axis].max(0.0);
                rect[axis + 2] = rect[axis + 2].min(content[axis]);
            }
            return (rect[0] < rect[2] && rect[1] < rect[3]).then_some(rect);
        };
        let ancestor = nodes.get(&id)?;
        let bounds = ancestor.bounds();
        let min = [bounds.min().x(), bounds.min().y()];
        let max = [bounds.max().x(), bounds.max().y()];
        for axis in 0..2 {
            rect[axis] += min[axis];
            rect[axis + 2] += min[axis];
            if ancestor.clips_children() {
                rect[axis] = rect[axis].max(min[axis]);
                rect[axis + 2] = rect[axis + 2].min(max[axis]);
            }
        }
        parent = ancestor.parent();
    }
    None
}

/// Which of a mod's screens a pointer belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Surface {
    View,
    Overlay,
}

/// The screen under GUI `point`: beside an open view, wherever the overlay may draw, the
/// overlay's, so its list stays usable beside the view; elsewhere the view's while it is open.
pub(super) fn pointer_surface(
    view_open: bool,
    layout: Option<&ScreenLayout>,
    point: [f64; 2],
) -> Surface {
    let beside = layout.is_some_and(|layout| layout.overlay_allows(point));
    if view_open && !beside {
        Surface::View
    } else {
        Surface::Overlay
    }
}

/// The collection name and index of the topmost enabled control at GUI `point` that sits in a
/// collection, by the rule `action` reports rows with (the control's nearest collection).
/// `regions` are `(rect, collection and index, enabled)` in draw order.
pub(super) fn nearest_row<'a>(
    regions: impl DoubleEndedIterator<Item = ([f64; 4], Option<(&'a str, usize)>, bool)>,
    [x, y]: [f64; 2],
) -> Option<(String, u32)> {
    regions
        .rev()
        .find_map(|([left, top, width, height], row, enabled)| {
            let inside = x >= left && y >= top && x < left + width && y < top + height;
            let (collection, index) = row.filter(|_| enabled && inside)?;
            Some((collection.to_owned(), u32::try_from(index).ok()?))
        })
}

/// The panels (the open view's while it is up) and exclusions in content-logical pixels,
/// `[left, top, right, bottom]`, at `scale` logical pixels per GUI unit.
pub(super) fn forbidden_logical(layout: &ScreenLayout, scale: f32) -> Vec<[f32; 4]> {
    std::iter::once(layout.panels())
        .chain(&layout.exclusions)
        .map(|rect| {
            [
                (rect.x as f32) * scale,
                (rect.y as f32) * scale,
                ((rect.x + rect.width) as f32) * scale,
                ((rect.y + rect.height) as f32) * scale,
            ]
        })
        .collect()
}

/// Drops every visible node from `start` on whose rect meets a `forbidden` one, so the overlay
/// never draws over vanilla's panels; empty clip groups stay as they draw nothing. The overlay's
/// `tooltips` (ranges into `nodes`) are kept whole and move above it, as vanilla's do.
pub(super) fn clip_overlay(
    nodes: &mut Vec<UiNode>,
    start: usize,
    forbidden: &[[f32; 4]],
    tooltips: &[Range<usize>],
    next: &mut u32,
) {
    let tooltips = take_lifted(nodes, tooltips);
    drop_forbidden(nodes, start, forbidden);
    restore_lifted(nodes, tooltips, next);
}

fn drop_forbidden(nodes: &mut Vec<UiNode>, start: usize, forbidden: &[[f32; 4]]) {
    let origins: BTreeMap<UiNodeId, [f32; 2]> = nodes
        .iter()
        .map(|node| {
            let min = node.bounds().min();
            (node.id(), [min.x(), min.y()])
        })
        .collect();
    let mut index = 0;
    nodes.retain(|node| {
        index += 1;
        if index <= start || matches!(node.visual(), UiVisual::None) {
            return true;
        }
        let origin = node
            .parent()
            .and_then(|parent| origins.get(&parent))
            .copied()
            .unwrap_or_default();
        let bounds = node.bounds();
        let rect = [
            origin[0] + bounds.min().x(),
            origin[1] + bounds.min().y(),
            origin[0] + bounds.max().x(),
            origin[1] + bounds.max().y(),
        ];
        !forbidden.iter().any(|area| {
            rect[0] < area[2] && area[0] < rect[2] && rect[1] < area[3] && area[1] < rect[3]
        })
    });
}

/// Nodes taken out of the frame to be drawn again on top, with the clip groups they sat in.
pub(super) struct Lifted {
    nodes: Vec<UiNode>,
    /// Each parent of a lifted node that stayed behind, to copy under the lifted node.
    groups: BTreeMap<UiNodeId, UiNode>,
}

/// Takes the nodes in `ranges` out of `nodes`, in order. A clip group among them that other
/// nodes still use stays and is copied instead.
pub(super) fn take_lifted(nodes: &mut Vec<UiNode>, ranges: &[Range<usize>]) -> Lifted {
    let lifted = |index: usize| ranges.iter().any(|range| range.contains(&index));
    let taken_ids: std::collections::BTreeSet<UiNodeId> = nodes
        .iter()
        .enumerate()
        .filter(|(index, _)| lifted(*index))
        .map(|(_, node)| node.id())
        .collect();
    // A taken node that parents a node left behind must stay.
    let kept_parents: std::collections::BTreeSet<UiNodeId> = nodes
        .iter()
        .filter(|node| !taken_ids.contains(&node.id()))
        .filter_map(UiNode::parent)
        .collect();
    let mut out = Lifted {
        nodes: Vec::new(),
        groups: BTreeMap::new(),
    };
    let mut index = 0;
    let all = std::mem::take(nodes);
    let by_id: BTreeMap<UiNodeId, UiNode> =
        all.iter().map(|node| (node.id(), node.clone())).collect();
    for node in all {
        let take = lifted(index) && !kept_parents.contains(&node.id());
        index += 1;
        if !take {
            if lifted(index - 1) {
                out.groups.insert(node.id(), node.clone());
            }
            nodes.push(node);
            continue;
        }
        if let Some(parent) = node.parent()
            && !taken_ids.contains(&parent)
            && let Some(group) = by_id.get(&parent)
        {
            out.groups.insert(parent, group.clone());
        }
        out.nodes.push(node);
    }
    // A kept group is copied, not moved: its lifted children follow the copy.
    for node in &out.nodes {
        if let Some(parent) = node.parent()
            && kept_parents.contains(&parent)
            && let Some(group) = by_id.get(&parent)
        {
            out.groups.insert(parent, group.clone());
        }
    }
    out
}

/// Appends the lifted nodes with fresh ids from `next`, so they draw after everything before
/// them; a lifted node whose group stayed behind gets a copy of that group first.
pub(super) fn restore_lifted(nodes: &mut Vec<UiNode>, lifted: Lifted, next: &mut u32) {
    let mut ids = BTreeMap::new();
    let mut fresh = || {
        let id = UiNodeId::new(*next);
        *next = next.saturating_add(1);
        id
    };
    for node in lifted.nodes {
        let parent = match node.parent() {
            None => None,
            Some(parent) if ids.contains_key(&parent) => ids.get(&parent).copied(),
            Some(parent) => match lifted.groups.get(&parent) {
                Some(group) => {
                    let copy = fresh();
                    nodes.push(group.clone().renumbered(copy, None));
                    ids.insert(parent, copy);
                    Some(copy)
                }
                None => Some(parent),
            },
        };
        let id = fresh();
        ids.insert(node.id(), id);
        nodes.push(node.renumbered(id, parent));
    }
}

/// Whether an overlay control at GUI `rect` (`[x, y, w, h]`) lies wholly outside the panels (the
/// open view's while it is up) and every exclusion, so it can never intercept them.
pub(super) fn overlay_hit_allowed(layout: &ScreenLayout, [x, y, w, h]: [f64; 4]) -> bool {
    std::iter::once(layout.panels())
        .chain(&layout.exclusions)
        .all(|area| {
            x + w <= area.x
                || area.x + area.width <= x
                || y + h <= area.y
                || area.y + area.height <= y
        })
}

/// Drops the overlay's hit regions that meet the GUI rect or an exclusion.
pub(super) fn keep_allowed_hits(frame: &mut EngineFrame, layout: &ScreenLayout) {
    let hits: Vec<_> = frame
        .hits
        .iter()
        .filter(|hit| overlay_hit_allowed(layout, [hit.rect.x, hit.rect.y, hit.rect.w, hit.rect.h]))
        .cloned()
        .collect();
    frame.hits = hits.into();
}

/// Window-logical `point` in the frame's GUI units.
pub(super) fn to_gui(frame: &EngineFrame, point: [f32; 2]) -> [f64; 2] {
    super::super::template_screen::virtual_point(frame, point)
}

#[cfg(test)]
mod tests;
