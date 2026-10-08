//! Reusing the last layout's output for subtrees whose controls and placement
//! inputs are unchanged. Each placed control records its inputs and where its
//! draws and hit regions sit relative to its parent's; a later layout that
//! reaches it with the same inputs splices that output instead of placing it.

use std::sync::Arc;

use super::measure::Memo;
use super::{LaidOut, LayoutEnv, Rect};
use crate::anim::Inherited;
use crate::emit::{DrawNode, RectOut};
use crate::input::{HitRegion, HitScope};
use crate::state::ViewState;

/// The root's record key; it moves between layouts, so its address cannot name it.
pub(super) const ROOT: usize = 0;

/// What a control is placed with, besides the layout-wide view state.
#[derive(Clone)]
pub(super) struct Inputs {
    pub(super) key: String,
    pub(super) rect: Rect,
    pub(super) parent_clip: Rect,
    pub(super) parent_rect: Rect,
    pub(super) layer: i32,
    pub(super) shown: bool,
    pub(super) packed: bool,
    pub(super) allows: bool,
    pub(super) enabled: bool,
    /// Whether every ancestor shows, so the subtree emits.
    pub(super) emitting: bool,
    pub(super) inherited: Inherited,
    pub(super) hits: Arc<HitScope>,
}

impl Inputs {
    fn same(&self, other: &Inputs) -> bool {
        self.key == other.key
            && self.rect == other.rect
            && self.parent_clip == other.parent_clip
            && self.parent_rect == other.parent_rect
            && self.layer == other.layer
            && self.shown == other.shown
            && self.packed == other.packed
            && self.allows == other.allows
            && self.enabled == other.enabled
            && self.emitting == other.emitting
            && self.inherited.same(&other.inherited)
            && (Arc::ptr_eq(&self.hits, &other.hits) || self.hits == other.hits)
    }
}

/// One control's last placement.
struct Record {
    inputs: Inputs,
    /// Nothing in the subtree reads context outside its inputs.
    cacheable: bool,
    /// The layout that last placed its children.
    placed: u64,
    /// The layout that last emitted it.
    seen: u64,
    /// Draws and hit regions, as `(offset from the parent's first, count)`.
    draws: (usize, usize),
    hits: (usize, usize),
    /// The subtree's last `button.menu_cancel` global target.
    cancel: Option<String>,
    /// The subtree's first visible `root_panel`.
    root_panel: Option<RectOut>,
}

/// Where a control's output sat in the last layout, and when it was placed.
#[derive(Clone, Copy)]
pub(super) struct Old {
    draws: usize,
    hits: usize,
    placed: u64,
}

/// A subtree spliced from the last layout.
#[derive(Clone, Debug)]
pub(crate) struct Reused {
    draws: std::ops::Range<usize>,
    hits: std::ops::Range<usize>,
    cancel: Option<String>,
    root_panel: Option<RectOut>,
}

/// The last layout's records and output, kept by a [`super::MeasureCache`].
#[derive(Default)]
pub(super) struct Placements {
    records: Memo<usize, Record>,
    generation: u64,
    /// The view state, culling and screen the records were made under.
    view: Option<(ViewState, bool, Rect)>,
    /// Controls the last layout placed rather than spliced.
    placed: usize,
    /// Draws in document order with their layers, and hit regions in order.
    draws: Vec<Option<(i32, DrawNode)>>,
    hits: Vec<Option<HitRegion>>,
}

impl Placements {
    /// Start a layout of a `screen` under `view`; another view state or screen
    /// invalidates every record.
    pub(super) fn begin(&mut self, view: &ViewState, cull: bool, screen: Rect) {
        if self
            .view
            .as_ref()
            .is_none_or(|(last, culled, size)| last != view || *culled != cull || *size != screen)
        {
            *self = Self {
                view: Some((view.clone(), cull, screen)),
                generation: self.generation,
                ..Self::default()
            };
        }
        self.generation += 1;
        self.placed = 0;
    }

    /// Controls the last layout placed rather than spliced.
    pub(super) fn placed_count(&self) -> usize {
        self.placed
    }

    /// A changed control can no longer be spliced, though its children may; a
    /// removed one's record goes.
    pub(super) fn forget(&mut self, address: usize, removed: bool) {
        if removed {
            self.records.remove(&address);
        } else if let Some(record) = self.records.get_mut(&address) {
            record.cacheable = false;
        }
    }

    /// Where `address` sat in the last layout, given where its parent did.
    pub(super) fn old(&self, address: usize, parent: Option<Old>) -> Option<Old> {
        let record = self.records.get(&address)?;
        let (draws, hits) = match parent {
            Some(parent) if record.seen == parent.placed => {
                (parent.draws + record.draws.0, parent.hits + record.hits.0)
            }
            None if address == ROOT && record.seen + 1 == self.generation => (0, 0),
            _ => return None,
        };
        Some(Old {
            draws,
            hits,
            placed: record.placed,
        })
    }

    /// The last output of `address` at `old`, when its inputs still match.
    pub(super) fn reuse(&self, address: usize, old: Old, inputs: &Inputs) -> Option<Reused> {
        let record = self.records.get(&address)?;
        (record.cacheable && record.inputs.same(inputs)).then(|| Reused {
            draws: old.draws..old.draws + record.draws.1,
            hits: old.hits..old.hits + record.hits.1,
            cancel: record.cancel.clone(),
            root_panel: record.root_panel,
        })
    }

    /// Record a control this layout placed; the output walk fills its spans.
    pub(super) fn placed(&mut self, address: usize, inputs: Inputs, cacheable: bool) {
        self.placed += 1;
        let generation = self.generation;
        let record = Record {
            inputs,
            cacheable,
            placed: generation,
            seen: 0,
            draws: (0, 0),
            hits: (0, 0),
            cancel: None,
            root_panel: None,
        };
        self.records.insert(address, record);
    }
}

/// The output of a layout over a [`Placements`].
pub(crate) struct Output {
    pub(crate) nodes: Vec<DrawNode>,
    pub(crate) hits: Vec<HitRegion>,
    pub(crate) cancel: Option<String>,
    pub(crate) root_panel: Option<RectOut>,
}

/// Emit `root`'s draws, hit regions, cancel target and `root_panel`, splicing
/// reused subtrees and recording every emitted control's spans.
pub(super) fn output(root: &LaidOut, env: &LayoutEnv, placements: &mut Placements) -> Output {
    let mut walk = Walk {
        env,
        previous_draws: std::mem::take(&mut placements.draws),
        previous_hits: std::mem::take(&mut placements.hits),
        placements,
        draws: Vec::new(),
        hits: Vec::new(),
        order: 0,
    };
    let (cancel, root_panel) = walk.node(root, ROOT, (0, 0), Some(&HitScope::default()));
    let Walk {
        draws,
        hits,
        placements,
        ..
    } = walk;
    let mut nodes: Vec<(i32, DrawNode)> = draws.clone();
    nodes.sort_by_key(|(layer, _)| *layer);
    let mut sorted_hits = hits.clone();
    sorted_hits.sort_by_key(|region| (region.layer, region.order));
    placements.draws = draws.into_iter().map(Some).collect();
    placements.hits = hits.into_iter().map(Some).collect();
    Output {
        nodes: nodes.into_iter().map(|(_, node)| node).collect(),
        hits: sorted_hits,
        cancel,
        root_panel,
    }
}

struct Walk<'p, 'e, 'x> {
    env: &'e LayoutEnv<'x>,
    placements: &'p mut Placements,
    previous_draws: Vec<Option<(i32, DrawNode)>>,
    previous_hits: Vec<Option<HitRegion>>,
    draws: Vec<(i32, DrawNode)>,
    hits: Vec<HitRegion>,
    order: usize,
}

impl Walk<'_, '_, '_> {
    /// Emit `node` (when `scope` is set, its ancestors all show), returning the
    /// subtree's cancel target and `root_panel`.
    fn node(
        &mut self,
        node: &LaidOut,
        address: usize,
        parent: (usize, usize),
        scope: Option<&HitScope>,
    ) -> (Option<String>, Option<RectOut>) {
        let start = (self.draws.len(), self.hits.len());
        let found = match &node.reused {
            Some(reused) => self.splice(reused),
            None => self.place(node, scope),
        };
        if let Some(record) = self.placements.records.get_mut(&address) {
            record.seen = self.placements.generation;
            record.draws = (start.0 - parent.0, self.draws.len() - start.0);
            record.hits = (start.1 - parent.1, self.hits.len() - start.1);
            record.cancel.clone_from(&found.0);
            record.root_panel = found.1;
        }
        found
    }

    fn place(
        &mut self,
        node: &LaidOut,
        scope: Option<&HitScope>,
    ) -> (Option<String>, Option<RectOut>) {
        let start = (self.draws.len(), self.hits.len());
        let shows = scope.filter(|_| node.visible);
        let mut cancel = None;
        let inner = shows.map(|scope| scope.inner(node.control, &node.key, node.rect));
        if let Some((inner, opened)) = &inner {
            let mut drawn = Vec::new();
            let mut order = 0;
            crate::emit::emit_own(node, self.env, &mut drawn, &mut order);
            self.draws
                .extend(drawn.into_iter().map(|(layer, _, node)| (layer, node)));
            self.hits
                .extend(crate::input::region(node, inner, *opened, &mut self.order));
            if node.control.properties.contains_key("button_mappings") {
                cancel =
                    crate::input::InputComponent::global_target(node.control, "button.menu_cancel");
            }
        }
        let mut root_panel =
            (node.control.name == "root_panel" && node.visible).then(|| node.rect.into());
        for child in &node.children {
            let address = std::ptr::from_ref(child.control).addr();
            let (child_cancel, child_panel) = self.node(
                child,
                address,
                start,
                inner.as_ref().map(|(inner, _)| &**inner),
            );
            cancel = child_cancel.or(cancel);
            root_panel = root_panel.or(child_panel);
        }
        (cancel, root_panel)
    }

    fn splice(&mut self, reused: &Reused) -> (Option<String>, Option<RectOut>) {
        for slot in &mut self.previous_draws[reused.draws.clone()] {
            self.draws.extend(slot.take());
        }
        for slot in &mut self.previous_hits[reused.hits.clone()] {
            if let Some(mut region) = slot.take() {
                region.order = self.order;
                self.order += 1;
                self.hits.push(region);
            }
        }
        (reused.cancel.clone(), reused.root_panel)
    }
}
