use bevy::prelude::Resource;

#[derive(Debug, Default, Resource)]
pub(crate) struct LocalMovementSpeedAuthority {
    session_id: u64,
    dimension: i32,
    last_sequence: Option<u64>,
    current: Option<f64>,
}

impl LocalMovementSpeedAuthority {
    pub(crate) fn begin_session(&mut self, session_id: u64, dimension: i32) {
        self.session_id = session_id;
        self.dimension = dimension;
        self.last_sequence = None;
        self.current = None;
    }

    pub(crate) fn replace_dimension(&mut self, session_id: u64, dimension: i32) {
        if session_id != self.session_id {
            return;
        }
        self.dimension = dimension;
        self.last_sequence = None;
        self.current = None;
    }

    pub(crate) fn apply(
        &mut self,
        session_id: u64,
        sequence: u64,
        dimension: i32,
        current: f64,
    ) -> bool {
        if session_id != self.session_id
            || dimension != self.dimension
            || self.last_sequence.is_some_and(|last| sequence <= last)
        {
            return false;
        }
        self.last_sequence = Some(sequence);
        if !current.is_finite() || current < 0.0 {
            return false;
        }
        self.current = Some(current);
        true
    }

    pub(crate) const fn current(&self) -> Option<f64> {
        self.current
    }
}

#[cfg(test)]
mod tests {
    use super::LocalMovementSpeedAuthority;

    #[test]
    fn authority_obeys_session_fifo_dimension_and_replacement_ordering() {
        let mut authority = LocalMovementSpeedAuthority::default();
        authority.begin_session(7, 0);
        assert!(authority.apply(7, 2, 0, 0.25));
        assert!(!authority.apply(6, 3, 0, 0.5));
        assert!(!authority.apply(7, 1, 0, 0.5));
        assert!(!authority.apply(7, 3, 1, 0.5));
        assert_eq!(authority.current(), Some(0.25));

        authority.replace_dimension(7, 1);
        assert_eq!(authority.current(), None);
        assert!(!authority.apply(7, 1, 0, 0.75));
        assert!(authority.apply(7, 1, 1, 0.0));
        assert_eq!(authority.current(), Some(0.0));

        authority.begin_session(8, -1);
        assert_eq!(authority.current(), None);
        assert!(!authority.apply(7, 2, -1, 1.0));
        assert!(authority.apply(8, 1, -1, 0.1));
    }

    #[test]
    fn invalid_updates_are_consumed_without_overwriting_last_valid_authority() {
        let mut authority = LocalMovementSpeedAuthority::default();
        authority.begin_session(1, 0);
        assert!(authority.apply(1, 1, 0, 0.2));
        for (sequence, value) in [(2, f64::NAN), (3, f64::INFINITY), (4, -0.1)] {
            assert!(!authority.apply(1, sequence, 0, value));
            assert_eq!(authority.current(), Some(0.2));
        }
        assert!(!authority.apply(1, 3, 0, 0.9));
        assert_eq!(authority.current(), Some(0.2));
    }
}
