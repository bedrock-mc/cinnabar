//! Native actor outlines share the existing depth-tested line renderer and primitive budget.

use super::super::State;
use mod_render::{LinePattern, Ribbon};

/// Refreshes outlines from focused current-session gameplay, independently of guest polling.
pub(in super::super) fn update(state: &mut State) {
    let mut ribbons = Vec::new();
    if state.grants.render
        && state.snapshot.is_some()
        && state.controls.frame.gameplay
        && state.controls.frame.focused
        && let Some(style) = state
            .hud
            .content
            .as_ref()
            .and_then(|hud| hud.actor_bounds.as_ref())
        && let Some(world) = &state.player_state.world
    {
        for observed in &state.player_state.observations.actors {
            let actor = &observed.actor;
            if actor.session != world.session
                || actor.dimension != world.dimension
                || actor.invisible
                || if actor.player {
                    !style.players || !state.grants.players
                } else {
                    !style.mobs || !state.grants.entities
                }
            {
                continue;
            }
            let distance = (actor.position.0 - world.position.0).powi(2)
                + (actor.position.1 - world.position.1).powi(2)
                + (actor.position.2 - world.position.2).powi(2);
            if distance > style.range.powi(2) {
                continue;
            }
            if let Some(bounds) = actor.boxes.first() {
                ribbons.push(outline(bounds, style.color));
            }
        }
    }
    state.render.native_lines(ribbons);
}

/// Walks every cube edge with three repeated edges and no diagonal connectors.
fn outline(bounds: &crate::ActorObservationBounds, color: [f32; 4]) -> Ribbon {
    let (x, y, z) = bounds.min;
    let (a, b, c) = bounds.max;
    let corners = [
        [x, y, z],
        [a, y, z],
        [a, y, c],
        [x, y, c],
        [x, b, z],
        [a, b, z],
        [a, b, c],
        [x, b, c],
    ];
    Ribbon {
        pattern: LinePattern::Solid,
        taper: false,
        width: 1.,
        color,
        points: [0, 1, 2, 3, 0, 4, 5, 1, 5, 6, 2, 6, 7, 3, 7, 4]
            .into_iter()
            .map(|i| corners[i])
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// A single strip stays within native limits and covers all twelve axis-aligned edges.
    #[test]
    fn cube_strip_has_all_edges_and_no_diagonals() {
        let bounds = crate::ActorObservationBounds {
            min: (0., 0., 0.),
            max: (1., 1., 1.),
        };
        let line = outline(&bounds, [1.; 4]);
        assert!(line.points.len() <= mod_api::MAX_RIBBON_POINTS);
        let mut edges = std::collections::BTreeSet::new();
        for edge in line.points.windows(2) {
            let a = edge[0].map(|n| n as u8);
            let b = edge[1].map(|n| n as u8);
            assert_eq!(a.iter().zip(b).filter(|(a, b)| **a != *b).count(), 1);
            edges.insert(if a < b { (a, b) } else { (b, a) });
        }
        assert_eq!(edges.len(), 12);
    }
}
