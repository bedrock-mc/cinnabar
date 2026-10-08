//! Animations as the 1.26.50 `UIAnimationComponent` runs them. Resolution links
//! each control's `@ns.anim` references (in `alpha`/`offset`/`size`/`uv`/`color`/
//! `clip_ratio` or `anims`, by name or inline) into an [`AnimGraph`]; layout gives
//! offset/size ends pixels; a caller-held [`Animator`] ticks each control's
//! instances at paint time, so layout never depends on the clock.

use serde_json::{Map, Value};

use crate::tree::ControlRef;

mod def;
mod ease;
mod layout;
mod paint;
mod runtime;

pub use def::{AnimGraph, AnimKind, AnimNode};
pub use ease::Easing;
pub(crate) use layout::Inherited;
pub use paint::{Animated, ControlAnims, NodeAnim};
pub use runtime::{AnimEvent, Animator, FlipWrite, Written};

/// Property holding a control's resolved [`AnimGraph`].
pub(crate) const GRAPH_KEY: &str = "anim_graph";
/// Property holding a factory instance's creation time in seconds.
pub(crate) const BORN_KEY: &str = "anim_born";
/// Property naming the caller clock that holds an instance's creation time, so
/// a re-sent title restarts its fade without re-binding the screen.
pub(crate) const CLOCK_KEY: &str = "anim_clock";
/// Properties whose value may be an animation reference or inline animation.
pub(crate) const ANIMATED_PROPERTIES: [&str; 6] =
    ["alpha", "uv", "color", "clip_ratio", "size", "offset"];
/// Most animation nodes one control links; a longer server-supplied graph is cut.
const MAX_NODES: usize = 256;

/// Loads a referenced definition, flattened and substituted, with its namespace.
pub(crate) type LoadDef<'a> = dyn FnMut(&ControlRef) -> Option<(Map<String, Value>, String)> + 'a;

/// Builds a control's graph; `load` returns a referenced definition flattened
/// and substituted, with its namespace.
pub(crate) struct GraphBuilder<'a> {
    graph: AnimGraph,
    names: Vec<(ControlRef, usize, Option<Value>)>,
    load: &'a mut LoadDef<'a>,
}

impl<'a> GraphBuilder<'a> {
    pub(crate) fn new(load: &'a mut LoadDef<'a>) -> Self {
        Self {
            graph: AnimGraph::default(),
            names: Vec::new(),
            load,
        }
    }

    /// Adds `value` (a reference or inline definition) as a head; returns the
    /// definition's initial value for a referencing property.
    pub(crate) fn add_head(&mut self, value: &Value, owner_ns: &str) -> Option<Option<Value>> {
        let (index, initial) = match value {
            Value::String(text) if text.starts_with('@') => {
                self.add_ref(&ControlRef::parse(text, owner_ns))?
            }
            Value::Object(props) if props.contains_key("anim_type") => {
                let index = self.add_props(props, owner_ns)?;
                let key = self.graph.nodes[index].kind.initial_key();
                (index, props.get(key).cloned())
            }
            _ => return None,
        };
        self.graph.heads.push(index);
        Some(initial)
    }

    fn add_ref(&mut self, target: &ControlRef) -> Option<(usize, Option<Value>)> {
        if let Some((_, index, initial)) = self.names.iter().find(|(name, ..)| name == target) {
            return Some((*index, initial.clone()));
        }
        let (props, namespace) = (self.load)(target)?;
        let index = self.push(&props)?;
        let key = self.graph.nodes[index].kind.initial_key();
        let initial = props.get(key).cloned();
        self.names.push((target.clone(), index, initial.clone()));
        self.link(index, &props, &namespace);
        Some((index, initial))
    }

    fn add_props(&mut self, props: &Map<String, Value>, owner_ns: &str) -> Option<usize> {
        let index = self.push(props)?;
        self.link(index, props, owner_ns);
        Some(index)
    }

    fn push(&mut self, props: &Map<String, Value>) -> Option<usize> {
        if self.graph.nodes.len() >= MAX_NODES {
            return None;
        }
        self.graph.nodes.push(AnimNode::parse(props)?);
        Some(self.graph.nodes.len() - 1)
    }

    fn link(&mut self, index: usize, props: &Map<String, Value>, namespace: &str) {
        let next = props
            .get("next")
            .and_then(Value::as_str)
            .filter(|text| text.starts_with('@'));
        if let Some(next) = next
            && let Some((target, _)) = self.add_ref(&ControlRef::parse(next, namespace))
        {
            self.graph.nodes[index].next = Some(target);
        }
    }

    pub(crate) fn finish(self) -> Option<AnimGraph> {
        (!self.graph.heads.is_empty()).then_some(self.graph)
    }
}
