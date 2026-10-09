//! Retained pointer edges for menu sounds, independent of action dispatch.

use launcher::menu::MenuAction;

/// OreUI waits before sounding a held touch; movement cancels that press.
const TOUCH_PRESS_SECONDS: f64 = 0.150;
const TOUCH_SLOP: f32 = 3.0;
use super::MAX_UI_TOUCHES;

#[derive(Clone, Copy, Debug)]
struct TouchPress<A> {
    id: u64,
    action: A,
    point: [f32; 2],
    started: f64,
    oreui: bool,
    sounded: bool,
    cancelled: bool,
}

/// Tracks touch sounds without allocating or replaying held inputs.
pub struct PressSounds<A = MenuAction> {
    touches: [Option<TouchPress<A>>; MAX_UI_TOUCHES],
}

impl<A: Copy> Default for PressSounds<A> {
    /// Starts with no captured touches.
    fn default() -> Self {
        Self {
            touches: [None; MAX_UI_TOUCHES],
        }
    }
}

impl<A: Copy + PartialEq> PressSounds<A> {
    /// Revokes presses when their screen loses input ownership.
    pub fn clear(&mut self) {
        self.touches.fill(None);
    }

    /// Revokes one cancelled finger while leaving other captured presses intact.
    pub fn cancel_touch(&mut self, id: u64) {
        if let Some(slot) = self
            .touches
            .iter_mut()
            .find(|slot| slot.is_some_and(|press| press.id == id))
        {
            *slot = None;
        }
    }

    /// Returns an accepted release even when its held press has already sounded.
    pub fn released_action(&self, id: u64, point: [f32; 2]) -> Option<A> {
        let press = self.touches.iter().flatten().find(|press| press.id == id)?;
        (!press.cancelled
            && (!press.oreui
                || ((point[0] - press.point[0]).abs() <= TOUCH_SLOP
                    && (point[1] - press.point[1]).abs() <= TOUCH_SLOP)))
            .then_some(press.action)
    }

    /// Emits a held OreUI touch after its delay, or an accepted tap on release.
    #[allow(clippy::too_many_arguments)]
    pub fn touch(
        &mut self,
        id: u64,
        action: Option<A>,
        point: [f32; 2],
        pressed: bool,
        held: bool,
        now: f64,
        oreui: bool,
    ) -> Option<A> {
        if pressed
            && let Some(action) = action
            && !self
                .touches
                .iter()
                .any(|slot| slot.is_some_and(|press| press.id == id))
        {
            let slot = self.touches.iter_mut().find(|slot| slot.is_none())?;
            *slot = Some(TouchPress {
                id,
                action,
                point,
                started: now,
                oreui,
                sounded: false,
                cancelled: false,
            });
        }
        let slot = self
            .touches
            .iter_mut()
            .find(|slot| slot.is_some_and(|press| press.id == id))?;
        let press = slot.as_mut()?;
        press.cancelled |= press.oreui
            && ((point[0] - press.point[0]).abs() > TOUCH_SLOP
                || (point[1] - press.point[1]).abs() > TOUCH_SLOP);
        let due = !held || (press.oreui && now >= press.started + TOUCH_PRESS_SECONDS);
        let accepted = held || action == Some(press.action);
        let emit = (!press.cancelled && !press.sounded && due && accepted).then_some(press.action);
        press.sounded |= emit.is_some();
        if !held {
            *slot = None;
        }
        emit
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use launcher::menu::MenuScreen;

    const ACTION: MenuAction = MenuAction::Navigate(MenuScreen::Settings);

    #[test]
    fn touch_press_sounds_once_and_quick_taps_sound_on_release() {
        let mut sounds = PressSounds::default();
        assert_eq!(
            sounds.touch(1, Some(ACTION), [0.0; 2], true, true, 1.0, true),
            None
        );
        assert_eq!(
            sounds.touch(1, Some(ACTION), [0.0; 2], false, true, 1.14, true),
            None
        );
        assert_eq!(
            sounds.touch(1, Some(ACTION), [0.0; 2], false, true, 1.16, true),
            Some(ACTION)
        );
        assert_eq!(
            sounds.touch(1, Some(ACTION), [0.0; 2], false, true, 2.0, true),
            None
        );
        assert_eq!(
            sounds.touch(1, Some(ACTION), [0.0; 2], false, false, 2.1, true),
            None
        );
        assert_eq!(
            sounds.touch(2, Some(ACTION), [0.0; 2], true, true, 3.0, true),
            None
        );
        assert_eq!(
            sounds.touch(2, Some(ACTION), [0.0; 2], false, false, 3.05, true),
            Some(ACTION)
        );
    }

    #[test]
    fn native_touch_sounds_at_its_press_deadline() {
        let mut sounds = PressSounds::default();
        sounds.touch(1, Some(ACTION), [0.0; 2], true, true, 1.0, true);
        assert_eq!(
            sounds.touch(
                1,
                Some(ACTION),
                [0.0; 2],
                false,
                true,
                1.0 + TOUCH_PRESS_SECONDS,
                true
            ),
            Some(ACTION)
        );
    }

    #[test]
    fn scrolling_and_lost_input_cancel_sound() {
        let mut sounds = PressSounds::default();
        sounds.touch(1, Some(ACTION), [0.0; 2], true, true, 1.0, true);
        assert_eq!(
            sounds.touch(1, Some(ACTION), [4.0, 0.0], false, true, 1.1, true),
            None
        );
        assert_eq!(
            sounds.touch(1, Some(ACTION), [0.0; 2], false, false, 1.2, true),
            None
        );
        sounds.touch(2, Some(ACTION), [0.0; 2], true, true, 2.0, true);
        sounds.clear();
        assert_eq!(
            sounds.touch(2, Some(ACTION), [0.0; 2], false, false, 2.1, true),
            None
        );
    }

    #[test]
    fn json_ui_touch_waits_for_release() {
        let mut sounds = PressSounds::default();
        sounds.touch(1, Some(ACTION), [0.0; 2], true, true, 1.0, false);
        assert_eq!(
            sounds.touch(1, Some(ACTION), [0.0; 2], false, true, 3.0, false),
            None
        );
        assert_eq!(
            sounds.touch(1, Some(ACTION), [0.0; 2], false, false, 3.1, false),
            Some(ACTION)
        );
    }

    #[test]
    fn json_ui_touch_uses_control_membership_instead_of_native_slop() {
        let mut sounds = PressSounds::default();
        sounds.touch(1, Some(ACTION), [0.0; 2], true, true, 1.0, false);
        assert_eq!(
            sounds.touch(1, Some(ACTION), [20.0, 0.0], false, false, 1.1, false),
            Some(ACTION)
        );
    }

    #[test]
    fn releasing_outside_the_control_does_not_sound() {
        let mut sounds = PressSounds::default();
        sounds.touch(1, Some(ACTION), [0.0; 2], true, true, 1.0, true);
        assert_eq!(
            sounds.touch(1, None, [1.0, 0.0], false, false, 1.05, true),
            None
        );
    }

    #[test]
    fn cancelling_one_touch_preserves_other_presses() {
        let mut sounds = PressSounds::default();
        for id in [1, 2] {
            sounds.touch(id, Some(ACTION), [0.0; 2], true, true, 1.0, true);
        }
        sounds.cancel_touch(1);
        assert_eq!(
            sounds.touch(1, Some(ACTION), [0.0; 2], false, false, 1.05, true),
            None
        );
        assert_eq!(
            sounds.touch(2, Some(ACTION), [0.0; 2], false, false, 1.05, true),
            Some(ACTION)
        );
    }

    #[test]
    fn idle_touch_state_does_not_allocate() {
        let mut sounds = PressSounds::<MenuAction>::default();
        let (_, count) = crate::allocation_count::count(|| {
            for _ in 0..100 {
                sounds.clear();
                assert_eq!(
                    sounds.touch(1, None, [0.0; 2], false, false, 0.0, true),
                    None
                );
            }
        });
        assert_eq!(count, 0);
    }
}
