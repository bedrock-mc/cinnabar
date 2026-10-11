use launcher::menu::MenuAction;
use launcher::menu::view::{SettingsFocusAxis, SettingsFocusLandmark, SettingsFocusTarget};
use ui::UiRect;

#[derive(Clone, Copy, Debug)]
enum Memory {
    Target(MenuAction),
    Landmark(u16),
}

#[derive(Default, Debug)]
pub(in crate::menu) struct SettingsFocusGeometry {
    pub(super) targets: Vec<SettingsFocusTarget>,
    pub(super) landmarks: Vec<SettingsFocusLandmark>,
    pub(super) native: bool,
    anchor: Option<(SettingsFocusAxis, MenuAction)>,
    remembered: Option<(MenuAction, Option<u16>)>,
    memory: Vec<(u16, Vec<Memory>)>,
}

impl SettingsFocusGeometry {
    pub(super) fn update(
        &mut self,
        targets: &[SettingsFocusTarget],
        landmarks: &[SettingsFocusLandmark],
    ) {
        let removed_traps = self
            .landmarks
            .iter()
            .filter(|previous| {
                previous.trap && !landmarks.iter().any(|next| next.id == previous.id)
            })
            .map(|landmark| landmark.id)
            .collect::<Vec<_>>();
        if !removed_traps.is_empty() {
            let removed = self
                .landmarks
                .iter()
                .filter(|landmark| {
                    removed_traps
                        .iter()
                        .any(|trap| landmark.id == *trap || self.descendant(landmark.parent, *trap))
                })
                .map(|landmark| landmark.id)
                .collect::<Vec<_>>();
            self.memory.retain(|(id, _)| !removed.contains(id));
            self.remembered = None;
            self.anchor = None;
        }
        self.native |= !landmarks.is_empty();
        self.targets.clear();
        self.targets.extend_from_slice(targets);
        self.landmarks.clear();
        self.landmarks.extend_from_slice(landmarks);
    }

    pub(super) fn reset_anchor(&mut self) {
        self.anchor = None;
    }

    pub(super) fn target(&self, action: MenuAction) -> Option<SettingsFocusTarget> {
        self.targets
            .iter()
            .find(|target| super::same_control(target.action, action))
            .copied()
    }

    fn landmark(&self, id: u16) -> Option<&SettingsFocusLandmark> {
        self.landmarks.iter().find(|landmark| landmark.id == id)
    }

    fn ancestors(&self, mut id: Option<u16>) -> Vec<u16> {
        let mut ancestors = Vec::new();
        while let Some(current) = id {
            if ancestors.contains(&current) {
                break;
            }
            ancestors.push(current);
            id = self.landmark(current).and_then(|landmark| landmark.parent);
        }
        ancestors
    }

    fn descendant(&self, parent: Option<u16>, ancestor: u16) -> bool {
        self.ancestors(parent).contains(&ancestor)
    }

    pub(super) fn remember(&mut self, action: MenuAction) {
        let Some(target) = self.target(action) else {
            return;
        };
        if self.remembered.is_some_and(|(previous, parent)| {
            super::same_control(previous, target.action) && parent == target.landmark
        }) {
            return;
        }
        self.remembered = Some((target.action, target.landmark));
        let mut child = Memory::Target(target.action);
        for id in self.ancestors(target.landmark) {
            if self
                .landmark(id)
                .is_some_and(|landmark| landmark.focus_control_disabled)
            {
                continue;
            }
            if self.landmark(id).is_some_and(|landmark| landmark.remember) {
                let index = self.memory.iter().position(|(key, _)| *key == id);
                let entries = if let Some(index) = index {
                    &mut self.memory[index].1
                } else {
                    self.memory.push((id, Vec::new()));
                    &mut self.memory.last_mut().expect("inserted memory").1
                };
                entries.retain(|entry| !same_memory(*entry, child));
                entries.insert(0, child);
            }
            child = Memory::Landmark(id);
        }
    }

    pub(super) fn entry(&self) -> Option<MenuAction> {
        self.landmarks
            .iter()
            .find(|landmark| landmark.parent.is_none())
            .and_then(|landmark| self.delegate(landmark.id, &mut Vec::new()))
            .or_else(|| self.targets.first().map(|target| target.action))
    }

    fn resolve_memory(
        &self,
        id: u16,
        memory: Memory,
        visited: &mut Vec<u16>,
    ) -> Option<MenuAction> {
        match memory {
            Memory::Target(action) => self
                .target(action)
                .filter(|target| self.descendant(target.landmark, id))
                .map(|target| target.action),
            Memory::Landmark(child) => self
                .landmark(child)
                .filter(|child| !child.focus_control_disabled && self.descendant(child.parent, id))
                .and_then(|child| self.delegate(child.id, visited)),
        }
    }

    fn delegate(&self, id: u16, visited: &mut Vec<u16>) -> Option<MenuAction> {
        if visited.contains(&id) {
            return None;
        }
        visited.push(id);
        let landmark = self.landmark(id)?;
        if landmark.remember
            && let Some((_, entries)) = self.memory.iter().find(|(key, _)| *key == id)
        {
            for entry in entries {
                if let Some(action) = self.resolve_memory(id, *entry, &mut visited.clone()) {
                    return Some(action);
                }
            }
        }
        if let Some(action) = landmark.delegate
            && let Some(action) =
                self.resolve_memory(id, Memory::Target(action), &mut visited.clone())
        {
            return Some(action);
        }
        if let Some(child) = landmark.delegate_landmark
            && let Some(action) =
                self.resolve_memory(id, Memory::Landmark(child), &mut visited.clone())
        {
            return Some(action);
        }
        for target in &self.targets {
            let ancestors = self.ancestors(target.landmark);
            let Some(index) = ancestors.iter().position(|ancestor| *ancestor == id) else {
                continue;
            };
            if index == 0 {
                return Some(target.action);
            }
            if let Some(action) = self.delegate(ancestors[index - 1], &mut visited.clone()) {
                return Some(action);
            }
        }
        None
    }

    pub(super) fn directional(
        &mut self,
        current: MenuAction,
        axis: SettingsFocusAxis,
        direction: i32,
    ) -> Option<MenuAction> {
        let Some(target) = self.target(current) else {
            return self.entry();
        };
        if !self.anchor.is_some_and(|(previous, _)| previous == axis) {
            self.anchor = Some((axis, target.action));
        }
        let anchor = self
            .anchor
            .and_then(|(_, action)| self.target(action))
            .unwrap_or(target);
        let ancestors = self.ancestors(target.landmark);
        let trap = ancestors
            .iter()
            .copied()
            .find(|id| self.landmark(*id).is_some_and(|landmark| landmark.trap));
        let scroll = ancestors.iter().copied().find(|id| {
            self.landmark(*id)
                .is_some_and(|landmark| landmark.scroll_axis.is_some())
        });
        let mut best = None;
        let mut best_score = f64::INFINITY;
        for candidate in &self.targets {
            if super::same_control(candidate.action, target.action)
                || !self.eligible(candidate.landmark, &ancestors, trap)
            {
                continue;
            }
            let score = score(
                target.bounds,
                candidate.bounds,
                anchor.bounds,
                axis,
                direction,
            )
            .map(|score| score * self.scroll_bias(candidate.landmark, scroll, axis));
            if let Some(score) = score
                && score < best_score
            {
                best_score = score;
                best = Some(candidate.action);
            }
        }
        for landmark in &self.landmarks {
            if landmark.focus_control_disabled
                || ancestors.contains(&landmark.id)
                || !self.eligible(landmark.parent, &ancestors, trap)
            {
                continue;
            }
            let Some(action) = self.delegate(landmark.id, &mut Vec::new()) else {
                continue;
            };
            let candidate_score = score(
                target.bounds,
                landmark.bounds,
                anchor.bounds,
                axis,
                direction,
            )
            .map(|score| score * self.scroll_bias(landmark.parent, scroll, axis));
            if let Some(score) = candidate_score
                && score < best_score
            {
                best_score = score;
                best = Some(action);
            }
        }
        best.or(Some(target.action))
    }

    fn eligible(&self, parent: Option<u16>, ancestors: &[u16], trap: Option<u16>) -> bool {
        trap.is_none_or(|trap| self.descendant(parent, trap))
            && self.ancestors(parent).iter().all(|ancestor| {
                ancestors.contains(ancestor)
                    || self
                        .landmark(*ancestor)
                        .is_some_and(|landmark| landmark.focus_control_disabled)
            })
    }

    fn scroll_bias(
        &self,
        parent: Option<u16>,
        scroll: Option<u16>,
        axis: SettingsFocusAxis,
    ) -> f64 {
        if scroll.is_some_and(|id| {
            self.descendant(parent, id)
                && self
                    .landmark(id)
                    .is_some_and(|landmark| landmark.scroll_axis == Some(axis))
        }) {
            0.000001
        } else {
            1.0
        }
    }
}

fn same_memory(a: Memory, b: Memory) -> bool {
    match (a, b) {
        (Memory::Target(a), Memory::Target(b)) => super::same_control(a, b),
        (Memory::Landmark(a), Memory::Landmark(b)) => a == b,
        _ => false,
    }
}

fn dimensions(bounds: UiRect, axis: SettingsFocusAxis) -> ([f64; 2], [f64; 2]) {
    let min = bounds.min();
    let max = bounds.max();
    match axis {
        SettingsFocusAxis::Horizontal => (
            [f64::from(min.x()), f64::from(max.x())],
            [f64::from(min.y()), f64::from(max.y())],
        ),
        SettingsFocusAxis::Vertical => (
            [f64::from(min.y()), f64::from(max.y())],
            [f64::from(min.x()), f64::from(max.x())],
        ),
    }
}

fn overlap(current: [f64; 2], candidate: [f64; 2]) -> f64 {
    (current[1].min(candidate[1]) - current[0].max(candidate[0])).max(0.0)
        / (current[1] - current[0])
}

fn logit(value: f64) -> f64 {
    (value / (1.0 - value)).ln()
}

fn score(
    current: UiRect,
    candidate: UiRect,
    anchor: UiRect,
    axis: SettingsFocusAxis,
    direction: i32,
) -> Option<f64> {
    let (main, perpendicular) = dimensions(current, axis);
    let (next_main, next_perpendicular) = dimensions(candidate, axis);
    let (_, anchor_perpendicular) = dimensions(anchor, axis);
    let edge = usize::from(direction > 0);
    let sign = if direction > 0 { -1.0 } else { 1.0 };
    let center_delta = ((main[0] + main[1]) - (next_main[0] + next_main[1])) * 0.5 * sign;
    let raw_gap = (main[edge] - next_main[1 - edge]) * sign;
    let gap = if raw_gap < 0.0 && center_delta > 0.0 {
        0.0
    } else {
        raw_gap
    };
    if gap < 0.0 {
        return None;
    }
    let span = (main[edge] - next_main[edge]) * sign;
    let anchor_delta = ((anchor_perpendicular[0] + anchor_perpendicular[1])
        - (next_perpendicular[0] + next_perpendicular[1]))
        * 0.5;
    let current_delta = ((perpendicular[0] + perpendicular[1])
        - (next_perpendicular[0] + next_perpendicular[1]))
        * 0.5;
    let perpendicular_overlap = overlap(perpendicular, next_perpendicular);
    let main_overlap = overlap(main, next_main);
    let missing_overlap = 1.0 - perpendicular_overlap;
    let lateral_ratio = (anchor_delta / if gap == 0.0 { 1.0 } else { gap }).abs();
    let limit = std::f64::consts::PI * 5.0 / 12.0;
    let overlap_angle = limit * main_overlap.powi(3);
    let anchored_angle = (lateral_ratio.atan() + overlap_angle) * missing_overlap;
    let current_angle = (current_delta / center_delta.abs()).abs().atan() + overlap_angle;
    let far_angle = (current_delta / if span == 0.0 { 1.0 } else { span })
        .abs()
        .atan();
    let outside = (span < 0.0 || far_angle >= limit) && perpendicular_overlap == 0.0;
    if anchored_angle >= limit && (current_angle >= limit || outside) {
        return None;
    }
    Some(
        gap.max(1.0)
            + lateral_ratio * missing_overlap
            + logit((missing_overlap + 1.0) * 0.5).min(10000.0)
            + logit((main_overlap + 1.0) * 0.5).min(7500.0),
    )
}

#[cfg(test)]
mod tests;
