//! Native pointer position survives focus changes until the pointer leaves the window.

pub struct Pointer {
    pub position: Option<(f32, f32)>,
    pub active: bool,
}

impl Default for Pointer {
    fn default() -> Self {
        Self {
            position: None,
            active: true,
        }
    }
}

impl Pointer {
    /// Hides hit testing while inactive and restores a stationary pointer on return.
    pub fn focus(&mut self, active: bool) {
        self.active = active;
    }

    /// Only the active window can offer a pointer target.
    pub fn hit_position(&self) -> Option<(f32, f32)> {
        self.position.filter(|_| self.active)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stationary_pointer_returns_after_native_focus_changes() {
        let mut pointer = Pointer {
            position: Some((12.0, 24.0)),
            ..Pointer::default()
        };
        let position = pointer.hit_position();
        pointer.focus(false);
        assert_eq!(pointer.hit_position(), None);
        pointer.focus(true);
        assert_eq!(pointer.hit_position(), position);
    }

    #[test]
    fn leaving_the_window_prevents_a_stale_target_on_focus_return() {
        let mut pointer = Pointer {
            position: Some((12.0, 24.0)),
            ..Pointer::default()
        };
        pointer.focus(false);
        pointer.position = None;
        pointer.focus(true);
        assert_eq!(pointer.hit_position(), None);
    }
}
