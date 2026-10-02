//! The caller-held animation state of a screen: one component per animated
//! control (keyed by its layout key), ticking its active instances as
//! `UIAnimationComponent::_animationTick` does and keeping the values they write.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use super::def::{AnimKind, AnimNode};
use super::paint::ControlAnims;

/// Floor under a duration when normalizing elapsed time.
const MIN_DURATION: f32 = 0.01;
/// Floor under a flip-book's frames per second.
const MIN_FPS: f32 = 0.1;
/// Most `next` links followed within one tick, against zero-length cycles.
const MAX_LINKS_PER_TICK: usize = 64;

/// What the animations report back to the screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnimEvent {
    /// A chain ended (or destroyed its control) with this `end_event` button id.
    End(String),
    /// `destroy_at_end` removed the control at this layout key.
    Destroy(String),
}

/// A flip-book's displayed frame, turned into uvs at paint time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlipWrite {
    pub frame: f32,
    pub count: i32,
    pub vertical: bool,
}

/// The property values a control's animations have written; unwritten ones
/// keep the control's static value.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Written {
    pub alpha: Option<f32>,
    pub clip: Option<f32>,
    pub color: Option<[f32; 4]>,
    /// Sprite uv origin in texture pixels.
    pub uv: Option<[f32; 2]>,
    pub flip: Option<FlipWrite>,
    /// An aseprite flip-book's elapsed milliseconds.
    pub aseprite_ms: Option<i64>,
    pub offset: Option<[f32; 2]>,
    pub size: Option<[f32; 2]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Playing,
    Waiting,
    WaitRender,
}

/// One node's instance state, shared by every head that reaches it.
#[derive(Clone, Copy, Debug)]
struct Instance {
    state: State,
    elapsed: f32,
    accum: f32,
    frame: f32,
    reversed: bool,
}

struct Component {
    anims: Arc<ControlAnims>,
    instances: Vec<Instance>,
    active: Vec<usize>,
    written: Written,
    rendered: bool,
    /// Clock value the component was created at, when set by the caller.
    born: Option<f64>,
    last: f64,
    touched: bool,
    destroyed: bool,
}

/// Retained animation state for the controls a caller paints.
#[derive(Default)]
pub struct Animator {
    components: HashMap<String, Component>,
    events: Vec<AnimEvent>,
    /// Button events fired this frame, replayed to components created after them.
    fired: Vec<String>,
    /// Roots removed by `destroy_at_end`, and whether a paint met them this frame.
    destroyed: BTreeMap<String, bool>,
    /// Creation time of controls without their own clock, when fixed.
    origin: Option<f64>,
}

fn under(key: &str, root: &str) -> bool {
    key.strip_prefix(root)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

fn initial_state(node: &AnimNode, rendered: bool) -> State {
    if node.play_event.is_some() {
        State::Waiting
    } else if node.wait_until_rendered && !rendered {
        State::WaitRender
    } else {
        State::Playing
    }
}

impl Animator {
    pub fn new() -> Self {
        Self::default()
    }

    /// An animator that treats every control as created at `origin`, for
    /// sampling one moment without prior frames.
    pub fn starting_at(origin: f64) -> Self {
        Self {
            origin: Some(origin),
            ..Self::default()
        }
    }

    /// The values `anims` writes at `now`, ticking its component once per clock value.
    pub fn sample(
        &mut self,
        anims: &Arc<ControlAnims>,
        now: f64,
        clocks: Option<&BTreeMap<String, f64>>,
    ) -> Written {
        if !anims.graph.valid() {
            return Written::default();
        }
        let born = anims
            .clock
            .as_ref()
            .and_then(|clock| clocks?.get(clock).copied())
            .or(anims.born)
            .or(self.origin);
        // A re-laid control keeps its instances while its program is the same.
        if let Some(component) = self.components.get_mut(anims.key.as_str())
            && component.born == born
            && (Arc::ptr_eq(&component.anims, anims) || component.anims.same_program(anims))
        {
            if !Arc::ptr_eq(&component.anims, anims) {
                component.anims = Arc::clone(anims);
            }
            return component.sample(now, &mut self.events, &mut self.destroyed);
        }
        let mut component = Component::new(Arc::clone(anims), born, now);
        for event in &self.fired {
            component.fire(event);
        }
        // Waiting nodes start at their first paint; other nodes may fast-forward.
        if !anims.disable_fast_forward
            && let Some(born) = born
            && now > born
        {
            component.tick((now - born) as f32, &mut self.events, &mut self.destroyed);
        }
        component.render();
        component.tick(0.0, &mut self.events, &mut self.destroyed);
        let written = component.sample(now, &mut self.events, &mut self.destroyed);
        self.components.insert(anims.key.clone(), component);
        written
    }

    /// Deliver button event `id`: plays or resets matching animations and
    /// resets components whose `animation_reset_name` it is.
    pub fn fire(&mut self, id: &str) {
        for component in self.components.values_mut() {
            component.fire(id);
        }
        self.fired.push(id.to_owned());
    }

    /// End and destroy events since the last call, in order.
    pub fn take_events(&mut self) -> Vec<AnimEvent> {
        std::mem::take(&mut self.events)
    }

    /// Whether `key` or an ancestor was removed by `destroy_at_end`.
    pub fn is_destroyed(&self, key: &str) -> bool {
        self.destroyed.keys().any(|root| under(key, root))
    }

    /// [`Animator::is_destroyed`] for a paint, which keeps the removal alive.
    pub(crate) fn hides(&mut self, key: &str) -> bool {
        let mut hidden = false;
        for (root, met) in &mut self.destroyed {
            if under(key, root) {
                *met = true;
                hidden = true;
            }
        }
        hidden
    }

    /// Drop components no paint touched since the last call, so a control that
    /// comes back is created afresh.
    pub fn end_frame(&mut self) {
        self.components
            .retain(|_, component| std::mem::replace(&mut component.touched, false));
        self.destroyed
            .retain(|_, met| std::mem::replace(met, false));
        self.fired.clear();
    }
}

impl Component {
    fn new(anims: Arc<ControlAnims>, born: Option<f64>, now: f64) -> Self {
        let instances = anims
            .graph
            .nodes
            .iter()
            .map(|node| Instance {
                state: initial_state(node, false),
                elapsed: 0.0,
                accum: 0.0,
                frame: 0.0,
                reversed: false,
            })
            .collect();
        Self {
            active: anims.graph.heads.clone(),
            anims,
            instances,
            written: Written::default(),
            rendered: false,
            born,
            last: now,
            touched: true,
            destroyed: false,
        }
    }

    /// Tick once per clock value, even when several draw nodes share this component.
    fn sample(
        &mut self,
        now: f64,
        events: &mut Vec<AnimEvent>,
        destroyed: &mut BTreeMap<String, bool>,
    ) -> Written {
        self.touched = true;
        let dt = now - self.last;
        if dt > 0.0 {
            self.last = now;
            self.tick(dt as f32, events, destroyed);
        } else if dt < 0.0 {
            self.last = now;
        }
        self.written
    }

    /// Resources are ready at the first paint (`onResourcesLoaded`).
    fn render(&mut self) {
        self.rendered = true;
        for instance in &mut self.instances {
            if instance.state == State::WaitRender {
                instance.state = State::Playing;
            }
        }
    }

    fn fire(&mut self, id: &str) {
        if self.destroyed {
            return;
        }
        if self.anims.reset_name.as_deref() == Some(id) {
            let graph = &self.anims.graph;
            self.active = graph
                .heads
                .iter()
                .copied()
                .filter(|head| graph.nodes[*head].resettable)
                .collect();
            for index in self.active.clone() {
                self.reset(index);
            }
            return;
        }
        for index in self.active.clone() {
            let node = &self.anims.graph.nodes[index];
            if node.play_event.as_deref() == Some(id) {
                self.instances[index].state = if node.wait_until_rendered && !self.rendered {
                    State::WaitRender
                } else {
                    State::Playing
                };
            } else if node.reset_event.as_deref() == Some(id) {
                self.reset(index);
            }
        }
    }

    /// An instance's `_reset`: back to its first moment, writing its start value.
    fn reset(&mut self, index: usize) {
        let node = &self.anims.graph.nodes[index];
        let instance = &mut self.instances[index];
        instance.state = initial_state(node, self.rendered);
        instance.elapsed = 0.0;
        let written = &mut self.written;
        match node.kind {
            AnimKind::Alpha => written.alpha = Some(node.from[0]),
            AnimKind::Clip if self.anims.has_sprite => written.clip = Some(node.from[0]),
            AnimKind::Color if self.anims.has_sprite => written.color = Some(node.from),
            AnimKind::Uv if self.anims.has_sprite => {
                written.uv = Some([node.from[0], node.from[1]]);
            }
            AnimKind::FlipBook => {
                instance.accum = 0.0;
                instance.frame = 0.0;
                written.flip = None;
                written.uv = Some([0.0, 0.0]);
            }
            AnimKind::Aseprite => {
                written.aseprite_ms = None;
                written.uv = Some([0.0, 0.0]);
            }
            AnimKind::Offset => written.offset = Some([node.from[0], node.from[1]]),
            AnimKind::Size => written.size = Some([node.from[0], node.from[1]]),
            _ => {}
        }
    }

    fn tick(
        &mut self,
        dt: f32,
        events: &mut Vec<AnimEvent>,
        destroyed: &mut BTreeMap<String, bool>,
    ) {
        if self.destroyed {
            return;
        }
        let mut slot = 0;
        while slot < self.active.len() {
            let mut index = self.active[slot];
            let mut step = dt;
            let mut links = 0;
            loop {
                if self.instances[index].state != State::Playing {
                    slot += 1;
                    break;
                }
                let Some(leftover) = self.step(index, step) else {
                    slot += 1;
                    break;
                };
                let node = &self.anims.graph.nodes[index];
                if let Some(name) = node.destroy_at_end.clone()
                    && let Some(key) = self.anims.ancestor_key(&name)
                {
                    if let Some(end) = &node.end_event {
                        events.push(AnimEvent::End(end.clone()));
                    }
                    events.push(AnimEvent::Destroy(key.clone()));
                    destroyed.insert(key, true);
                    self.destroyed = true;
                    return;
                }
                match node.next {
                    Some(next) => {
                        self.instances[next].elapsed = 0.0;
                        self.active[slot] = next;
                        links += 1;
                        if links >= MAX_LINKS_PER_TICK {
                            slot += 1;
                            break;
                        }
                        index = next;
                        step = leftover;
                    }
                    None => {
                        if let Some(end) = &node.end_event {
                            events.push(AnimEvent::End(end.clone()));
                        }
                        self.active.remove(slot);
                        break;
                    }
                }
            }
        }
    }

    /// Advance one instance by `dt`; `Some(leftover)` once it has finished.
    fn step(&mut self, index: usize, dt: f32) -> Option<f32> {
        let node = &self.anims.graph.nodes[index];
        let has_sprite = self.anims.has_sprite;
        let rest_alpha = self.anims.rest_alpha;
        let instance = &mut self.instances[index];
        let written = &mut self.written;
        match node.kind {
            AnimKind::FlipBook => return flip_step(node, instance, written, dt),
            AnimKind::Aseprite => {
                instance.accum += dt;
                written.aseprite_ms = Some((instance.accum * 1000.0) as i32 as i64);
                return None;
            }
            // Sprite animations on a control without a sprite end at once.
            AnimKind::Clip | AnimKind::Color | AnimKind::Uv if !has_sprite => return Some(dt),
            _ => {}
        }
        instance.elapsed += dt;
        // A zero duration ends on the first frame, whose delta always exceeds the floor.
        let raw = if node.duration <= 0.0 {
            1.0
        } else {
            instance.elapsed / node.duration.max(MIN_DURATION)
        };
        let t = raw.clamp(0.0, 1.0);
        let ease = |channel: usize| node.easing.apply(node.from[channel], node.to[channel], t);
        match node.kind {
            AnimKind::Alpha => {
                let start = if node.scale_from_starting_alpha {
                    rest_alpha
                } else {
                    1.0
                };
                written.alpha = Some(ease(0) * start);
            }
            AnimKind::Clip => written.clip = Some(ease(0)),
            AnimKind::Color => written.color = Some([ease(0), ease(1), ease(2), ease(3)]),
            AnimKind::Uv => written.uv = Some([ease(0), ease(1)]),
            AnimKind::Offset => written.offset = Some([ease(0), ease(1)]),
            AnimKind::Size => written.size = Some([ease(0), ease(1)]),
            _ => {}
        }
        (raw >= 1.0).then(|| (instance.elapsed - node.duration.max(0.0)).max(0.0))
    }
}

/// `UIAnimFlipbook::tick`: at most one frame per tick, the remainder kept.
fn flip_step(
    node: &AnimNode,
    instance: &mut Instance,
    written: &mut Written,
    dt: f32,
) -> Option<f32> {
    let count = node.frame_count as f32;
    let next = instance.frame + 1.0;
    if count <= next && !node.looping {
        if !node.resettable {
            return Some(0.0);
        }
        instance.state = State::Waiting;
        return None;
    }
    let spf = 1.0 / node.fps.max(MIN_FPS);
    instance.accum += dt;
    if spf <= instance.accum {
        instance.accum -= spf;
        instance.frame = next;
        if count <= next {
            instance.frame = 0.0;
            if node.reversible {
                instance.reversed = !instance.reversed;
            }
        }
    }
    let frame = if instance.reversed {
        (count - instance.frame) + -1.0
    } else {
        instance.frame
    };
    written.flip = Some(FlipWrite {
        frame,
        count: node.frame_count,
        vertical: node.vertical,
    });
    None
}
