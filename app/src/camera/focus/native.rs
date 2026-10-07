//! Releases Windows desktop ownership in the native focus transition.

#[derive(Clone, Copy)]
struct NativeFocus(usize);

impl NativeFocus {
    /// Encodes the native activation, keyboard focus and visibility flags.
    fn new(active: bool, focused: bool, minimized: bool) -> Self {
        Self(usize::from(active) | (usize::from(focused) << 1) | (usize::from(minimized) << 2))
    }

    /// Reports whether the native window can own desktop mouse input.
    fn available(self) -> bool {
        self.0 & 7 == 3
    }

    /// Releases once on loss, including when later notifications repeat that loss.
    fn change(&mut self, bit: usize, enabled: bool) -> bool {
        let available = self.available();
        self.0 = (self.0 & !bit) | if enabled { bit } else { 0 };
        available && !self.available()
    }
}

#[cfg(all(windows, not(test)))]
mod windows;
#[cfg(all(windows, not(test)))]
pub(crate) use windows::NativeCaptureReady;
#[cfg(all(windows, not(test)))]
pub(super) use windows::install;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deactivation_releases_before_later_focus_loss() {
        let mut focus = NativeFocus::new(true, true, false);
        assert!(focus.change(1, false));
        // Another application may establish its clip before the later notification.
        assert!(!focus.change(2, false));
        assert!(!focus.change(1, false));
        assert!(!focus.change(4, true));
    }

    #[test]
    fn keyboard_loss_releases_even_without_logical_button_state() {
        let mut focus = NativeFocus::new(true, true, false);
        assert!(focus.change(2, false));
        assert!(!focus.change(1, false));
        assert!(!focus.change(2, true));
        assert!(!focus.change(1, true));
        assert!(focus.change(2, false));
    }

    #[test]
    fn minimization_releases_only_on_the_available_transition() {
        let mut focus = NativeFocus::new(true, true, false);
        assert!(focus.change(4, true));
        assert!(!focus.change(4, true));
        assert!(!focus.change(4, false));
        assert!(focus.change(1, false));
        let mut background = NativeFocus::new(false, false, false);
        assert!(!background.change(1, false));
    }
}
