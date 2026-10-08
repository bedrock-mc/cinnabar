//! What a laid-out control takes from its ancestors for animation: the factory
//! creation clock, `propagate_alpha` contributors, and moving ancestors.

use std::sync::Arc;

use serde_json::Value;

use super::def::AnimKind;
use super::paint::{ControlAnims, NodeAnim};
use super::{BORN_KEY, CLOCK_KEY};
use crate::tree::ResolvedControl;

#[derive(Clone, Default)]
pub(crate) struct Inherited {
    /// A `propagate_alpha` ancestor chain: static product of its unanimated
    /// alphas, its full static product, and its animated contributors.
    alpha: Option<(f32, f32)>,
    alpha_anims: Vec<Arc<ControlAnims>>,
    born: Option<f64>,
    clock: Option<String>,
    movers: Vec<Arc<ControlAnims>>,
    clip_movers: Vec<Arc<ControlAnims>>,
}

/// A control's own animation context before layout gives it pixels.
pub(crate) struct Own {
    pub(crate) born: Option<f64>,
    pub(crate) clock: Option<String>,
}

impl Inherited {
    /// Whether two contexts animate a subtree alike.
    pub(crate) fn same(&self, other: &Inherited) -> bool {
        let same = |a: &[Arc<ControlAnims>], b: &[Arc<ControlAnims>]| {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| Arc::ptr_eq(a, b) || a == b)
        };
        self.alpha == other.alpha
            && self.born == other.born
            && self.clock == other.clock
            && same(&self.alpha_anims, &other.alpha_anims)
            && same(&self.movers, &other.movers)
            && same(&self.clip_movers, &other.clip_movers)
    }

    /// The creation clock `control` animates on.
    pub(crate) fn own(&self, control: &ResolvedControl) -> Own {
        Own {
            born: crate::widgets::bound_number(control, BORN_KEY).or(self.born),
            clock: control
                .properties
                .get(CLOCK_KEY)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| self.clock.clone()),
        }
    }

    /// The control's static alpha, its draws' animations, and what its children inherit.
    pub(crate) fn apply(
        &self,
        control: &ResolvedControl,
        rest: f32,
        own: Option<Arc<ControlAnims>>,
        clips: bool,
        sprite: impl FnOnce(&mut NodeAnim),
    ) -> (f32, Option<Arc<NodeAnim>>, Inherited) {
        let context = self.own(control);
        let fades = own.as_ref().is_some_and(|own| own.writes(AnimKind::Alpha));
        let (unanimated, full) = self.alpha.unwrap_or((1.0, 1.0));
        let unanimated = if fades { unanimated } else { unanimated * rest };
        let full = full * rest;
        let mut alpha_anims = self.alpha_anims.clone();
        let mut movers = self.movers.clone();
        if let Some(own) = &own {
            if fades {
                alpha_anims.push(Arc::clone(own));
            }
            if own.moves() {
                movers.push(Arc::clone(own));
            }
        }
        let animated = own.is_some()
            || !alpha_anims.is_empty()
            || !movers.is_empty()
            || !self.clip_movers.is_empty();
        let node = animated.then(|| {
            let mut node = NodeAnim {
                own: own.clone(),
                alpha: alpha_anims.clone(),
                alpha_static: unanimated,
                movers: movers.clone(),
                clip_movers: self.clip_movers.clone(),
                ..NodeAnim::default()
            };
            sprite(&mut node);
            Arc::new(node)
        });
        let propagate = matches!(
            control.properties.get("propagate_alpha"),
            Some(Value::Bool(true))
        );
        let children = Inherited {
            alpha: if propagate {
                Some((unanimated, full))
            } else {
                self.alpha
            },
            alpha_anims: if propagate {
                alpha_anims
            } else {
                self.alpha_anims.clone()
            },
            born: context.born,
            clock: context.clock,
            clip_movers: if clips {
                movers.clone()
            } else {
                self.clip_movers.clone()
            },
            movers,
        };
        (full, node, children)
    }
}
