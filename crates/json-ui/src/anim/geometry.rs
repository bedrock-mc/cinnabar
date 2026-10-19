//! Explicit layout projection for simple automatic size or offset entrance chains.
//! Other components retain their ordinary paint-time lifecycle and resource readiness.

use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use serde_json::{Value, json};

use super::{Animator, ControlAnims, GEOMETRY_KEY, GRAPH_KEY};
use crate::{FormRender, LaidOut, LayoutEnv, MeasureCache, ResolvedControl, ViewState};

#[cfg(test)]
mod tests;

struct Program {
    path: Vec<usize>,
    animation: Arc<ControlAnims>,
    gates: Vec<crate::emit::StateGate>,
    property: &'static str,
    baseline: Option<Value>,
}

/// A bound screen's geometry animation programs and its latest measured frame.
/// Recreate it when the bound screen, viewport or measurement environment changes.
pub struct GeometryLayout {
    programs: Vec<Program>,
    owned_controls: Vec<String>,
    root: [f64; 2],
    view: ViewState,
    measures: MeasureCache,
    rendered: Option<Arc<FormRender>>,
}

impl GeometryLayout {
    /// Borrows the last published frame for diagnostics without advancing its animations.
    pub fn current(&self) -> &FormRender {
        self.rendered.as_deref().expect("geometry frame retained")
    }

    /// Captures automatic entrances before culling can omit an entering control.
    /// Returns none when every component needs the ordinary paint-time lifecycle.
    pub fn new(
        bound: &ResolvedControl,
        root: [f64; 2],
        env: &LayoutEnv,
        view: &ViewState,
    ) -> Option<Self> {
        if !has_entrance(bound) {
            return None;
        }
        let mut paths = HashMap::new();
        control_paths(bound, &mut Vec::new(), &mut paths);
        let (laid, _) = crate::layout_with(bound, root, env, &view.layout_part());
        let mut programs = Vec::new();
        let mut owned_controls = Vec::new();
        collect(&laid, &paths, &mut programs, &mut owned_controls);
        if programs.is_empty() {
            return None;
        }
        let mut bound = bound.clone();
        for program in &programs {
            at(&mut bound, &program.path)
                .properties
                .insert(GEOMETRY_KEY.into(), Value::Bool(true));
        }
        let mut measures = MeasureCache::default();
        let rendered = crate::render_bound_cached(bound, root, env, view, &mut measures);
        Some(Self {
            programs,
            owned_controls,
            root,
            view: view.clone(),
            measures,
            rendered: Some(Arc::new(rendered)),
        })
    }

    /// Samples caller-supplied time before measuring changed geometry. Unchanged writes
    /// reuse the complete frame, including its clips, scroll extents and hit regions.
    /// Interaction-hidden programs stop; clipped entering controls still advance.
    pub fn sample(
        &mut self,
        animator: &mut Animator,
        now: f64,
        clocks: Option<&BTreeMap<String, f64>>,
        env: &LayoutEnv,
        view: &ViewState,
    ) -> Arc<FormRender> {
        // Form ownership keeps removals alive independently of sampling or emitted primitives.
        for key in &self.owned_controls {
            animator.hides(key);
        }
        let mut writes = Vec::with_capacity(self.programs.len());
        for program in &self.programs {
            let hidden = animator.is_destroyed(&program.animation.key);
            let value = if !hidden && program.gates.iter().all(|gate| gate.shown(view)) {
                animator
                    .sample_entrance(&program.animation, now, clocks)
                    .map(|sample| {
                        layout_value(&program.animation.graph.nodes[sample.node], sample.progress)
                    })
                    .or_else(|| program.baseline.clone())
            } else {
                program.baseline.clone()
            };
            writes.push(value);
        }
        let saved = self.rendered.take().expect("geometry frame retained");
        let changed = self.programs.iter().zip(&writes).any(|(program, value)| {
            at_ref(&saved.bound, &program.path)
                .properties
                .get(program.property)
                != value.as_ref()
        });
        let rendered = if changed || self.view != *view {
            let mut saved = Arc::unwrap_or_clone(saved);
            for (program, value) in self.programs.iter().zip(writes) {
                let properties = &mut at(&mut saved.bound, &program.path).properties;
                match value {
                    Some(value) => {
                        properties.insert(program.property.into(), value);
                    }
                    None => {
                        properties.remove(program.property);
                    }
                }
            }
            self.view = view.clone();
            // Measurements belong to this frame's dimensions, never historical extents.
            self.measures = MeasureCache::default();
            Arc::new(crate::render_bound_cached(
                saved.bound,
                self.root,
                env,
                view,
                &mut self.measures,
            ))
        } else {
            saved
        };
        self.rendered = Some(Arc::clone(&rendered));
        rendered
    }
}

/// Interpolates authored coefficients so percentages keep following current layout dimensions.
fn layout_value(node: &super::AnimNode, progress: f32) -> Value {
    Value::Array(
        (0..2)
            .map(|axis| {
                node.from_expr
                    .get(axis)
                    .zip(node.to_expr.get(axis))
                    .and_then(|(from, to)| {
                        crate::length_from_value(from).blend(
                            &crate::length_from_value(to),
                            |from, to| {
                                f64::from(node.easing.apply(from as f32, to as f32, progress))
                            },
                        )
                    })
                    .map_or_else(
                        || json!(node.easing.apply(node.from[axis], node.to[axis], progress)),
                        |length| length.to_value(),
                    )
            })
            .collect(),
    )
}

/// Whether this subtree can contain an automatic entrance before placement checks its context.
fn has_entrance(control: &ResolvedControl) -> bool {
    control
        .properties
        .get(GRAPH_KEY)
        .and_then(|value| serde_json::from_value::<super::AnimGraph>(value.clone()).ok())
        .is_some_and(|graph| graph.entrance_property().is_some())
        || control.children.iter().any(has_entrance)
}

/// Associates immutable control addresses with paths in the owned bound tree.
fn control_paths(
    control: &ResolvedControl,
    path: &mut Vec<usize>,
    out: &mut HashMap<usize, Vec<usize>>,
) {
    out.insert(std::ptr::from_ref(control).addr(), path.clone());
    for (index, child) in control.children.iter().enumerate() {
        path.push(index);
        control_paths(child, path, out);
        path.pop();
    }
}

/// Captures all interaction states before clipping, including controls without primitives.
fn collect(
    laid: &LaidOut,
    paths: &HashMap<usize, Vec<usize>>,
    out: &mut Vec<Program>,
    owned: &mut Vec<String>,
) {
    crate::emit::visit_gated(laid, |laid, gates| {
        owned.push(laid.key.clone());
        if let Some(context) = &laid.anim
            && let Some(own) = &context.own
            && let Some(property) = own.entrance_property()
            && context
                .movers
                .iter()
                .all(|mover| mover.entrance_property().is_some())
        {
            out.push(Program {
                path: paths[&std::ptr::from_ref(laid.control).addr()].clone(),
                animation: Arc::clone(own),
                gates: gates.to_vec(),
                property,
                baseline: laid.control.properties.get(property).cloned(),
            });
        }
    });
}

/// Borrows one control by its stable child-index path.
fn at_ref<'a>(root: &'a ResolvedControl, path: &[usize]) -> &'a ResolvedControl {
    path.iter()
        .fold(root, |control, &index| &control.children[index])
}

/// Edits one control by its stable child-index path.
fn at<'a>(root: &'a mut ResolvedControl, path: &[usize]) -> &'a mut ResolvedControl {
    path.iter()
        .fold(root, |control, &index| &mut control.children[index])
}
