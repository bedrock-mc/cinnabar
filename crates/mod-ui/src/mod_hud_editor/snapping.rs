//! Allocation-free nearest placement guides in safe-content GUI pixels.

use super::*;

pub(super) const GRID: f64 = 8.;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Target {
    pub at: f64,
    pub guide: f64,
    pub card: Option<usize>,
}

/// Streams candidates once, retaining only the nearest admissible target on each axis.
pub(super) fn targets(
    hud: &Hud,
    index: usize,
    at: [f64; 2],
    viewport: [f64; 2],
    keep_out: &[[f64; 4]],
    bypass: bool,
    hidden: &std::collections::HashSet<String>,
) -> [Option<Target>; 2] {
    let mut best: [Option<Target>; 2] = [None; 2];
    if bypass {
        return best;
    }
    let size = cards::dimensions(&hud.cards[index]);
    let mut consider = |axis: usize, target: Target| {
        let distance = (at[axis] - target.at).abs();
        if target.at >= 0.
            && target.at <= (viewport[axis] - size[axis]).max(0.)
            && distance <= AXIS_SNAP_DISTANCE
            && best[axis].is_none_or(|old| distance < (at[axis] - old.at).abs())
        {
            best[axis] = Some(target);
        }
    };
    for axis in 0..2 {
        for guide in [0., viewport[axis] * 0.5, viewport[axis]] {
            for fraction in [0., 0.5, 1.] {
                consider(
                    axis,
                    Target {
                        at: guide - size[axis] * fraction,
                        guide,
                        card: None,
                    },
                );
            }
        }
    }
    for (other, card) in hud
        .cards
        .iter()
        .enumerate()
        .filter(|(other, card)| *other != index && !hidden.contains(&card.id))
    {
        let origin = cards::origin(card, viewport);
        let extent = cards::dimensions(card);
        if (0..2).any(|axis| origin[axis] >= viewport[axis] || origin[axis] + extent[axis] <= 0.) {
            continue;
        }
        for axis in 0..2 {
            for guide in [origin[axis], origin[axis] + extent[axis]] {
                for edge in [0., size[axis]] {
                    consider(
                        axis,
                        Target {
                            at: guide - edge,
                            guide,
                            card: Some(other),
                        },
                    );
                }
            }
        }
        if at[0] < origin[0] + extent[0] && at[0] + size[0] > origin[0] {
            for (target, guide) in [
                (origin[1] - GRID - size[1], origin[1] - GRID),
                (origin[1] + extent[1] + GRID, origin[1] + extent[1] + GRID),
            ] {
                consider(
                    1,
                    Target {
                        at: target,
                        guide,
                        card: Some(other),
                    },
                );
            }
        }
    }
    for rect in keep_out {
        for axis in 0..2 {
            for guide in [rect[axis], rect[axis] + rect[axis + 2]] {
                for edge in [0., size[axis]] {
                    consider(
                        axis,
                        Target {
                            at: guide - edge,
                            guide,
                            card: None,
                        },
                    );
                }
            }
        }
    }
    best
}
