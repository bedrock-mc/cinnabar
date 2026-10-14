//! Which dimensions' store sessions are begun, and when they end and begin again.

use lod_store::{actor::Request, identity::Dimension, minute::Minute};

/// The store sessions one stream writes into. Every dimension change begins a session
/// again, since a proxy may have switched worlds behind it; a session left behind ends,
/// and one about to begin again first ends, once no captured column still waits to be
/// written, so a visit's evictions never land in the next visit's epoch.
#[derive(Debug, Default)]
pub(super) struct DimensionSessions {
    /// The dimension whose session the stream writes into.
    current: Option<Dimension>,
    /// The stream's dimension change the session began after; see
    /// [`chunk_pipeline::WorldStream::form_dimension_epoch`].
    epoch: u64,
    /// The current dimension's session begins again once ingestion settles.
    restart_due: bool,
    /// Dimensions whose sessions end once ingestion settles, in the order they were left.
    ending: Vec<Dimension>,
}

impl DimensionSessions {
    /// Follows the stream into `dimension` after its dimension change `epoch`; `settled`
    /// says no captured column still waits to be written. Returns the session requests
    /// to send, in order, stamped with `now`.
    pub(super) fn follow(
        &mut self,
        dimension: Dimension,
        epoch: u64,
        settled: bool,
        now: Minute,
    ) -> Vec<Request> {
        let mut requests = Vec::new();
        if self.current != Some(dimension) {
            if let Some(previous) = self.current.replace(dimension)
                && !self.ending.contains(&previous)
            {
                self.ending.push(previous);
            }
            // A dimension whose earlier session still waits for its writes begins again
            // only after that session ends.
            self.restart_due = self.ending.contains(&dimension);
            if !self.restart_due {
                requests.push(Request::Begin { dimension, now });
            }
        } else if self.epoch != epoch {
            self.restart_due = true;
        }
        self.epoch = epoch;
        if settled {
            for ending in self.ending.drain(..) {
                requests.push(Request::End { dimension: ending });
            }
            if std::mem::take(&mut self.restart_due) {
                requests.push(Request::Begin { dimension, now });
            }
        }
        requests
    }

    /// The dimension the stream writes into.
    pub(super) fn current(&self) -> Option<Dimension> {
        self.current
    }

    /// Dimensions with a session begun and not yet ended.
    pub(super) fn dimensions(&self) -> impl Iterator<Item = Dimension> + '_ {
        self.current.into_iter().chain(
            self.ending
                .iter()
                .copied()
                .filter(|ending| Some(*ending) != self.current),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: Minute = Minute::new(1);
    const OVERWORLD: Dimension = Dimension(0);
    const NETHER: Dimension = Dimension(1);

    /// A session request: a begin stamped with [`NOW`], or an end.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Session {
        Begin(Dimension),
        End(Dimension),
    }
    use Session::{Begin, End};

    fn follow(
        sessions: &mut DimensionSessions,
        dimension: Dimension,
        epoch: u64,
        settled: bool,
    ) -> Vec<Session> {
        sessions
            .follow(dimension, epoch, settled, NOW)
            .into_iter()
            .map(|request| match request {
                Request::Begin { dimension, now } => {
                    assert_eq!(now, NOW);
                    Begin(dimension)
                }
                Request::End { dimension } => End(dimension),
                other => panic!("not a session request: {other:?}"),
            })
            .collect()
    }

    #[test]
    fn a_new_dimension_begins_at_once_and_the_left_one_ends_once_settled() {
        let mut sessions = DimensionSessions::default();
        assert_eq!(
            follow(&mut sessions, OVERWORLD, 0, true),
            [Begin(OVERWORLD)]
        );
        assert_eq!(follow(&mut sessions, NETHER, 1, false), [Begin(NETHER)]);
        assert_eq!(
            sessions.dimensions().collect::<Vec<_>>(),
            [NETHER, OVERWORLD]
        );
        assert_eq!(follow(&mut sessions, NETHER, 1, true), [End(OVERWORLD)]);
        assert_eq!(sessions.dimensions().collect::<Vec<_>>(), [NETHER]);
    }

    #[test]
    fn returning_before_the_old_visit_is_written_waits_to_begin_again() {
        let mut sessions = DimensionSessions::default();
        follow(&mut sessions, OVERWORLD, 0, true);
        follow(&mut sessions, NETHER, 1, false);
        assert!(
            follow(&mut sessions, OVERWORLD, 2, false).is_empty(),
            "the first visit's evictions are still unwritten"
        );
        assert_eq!(
            sessions.dimensions().collect::<Vec<_>>(),
            [OVERWORLD, NETHER]
        );
        assert_eq!(
            follow(&mut sessions, OVERWORLD, 2, true),
            [End(OVERWORLD), End(NETHER), Begin(OVERWORLD)]
        );
        assert!(follow(&mut sessions, OVERWORLD, 2, true).is_empty());
    }

    #[test]
    fn a_change_into_the_same_dimension_begins_again_once_settled() {
        let mut sessions = DimensionSessions::default();
        follow(&mut sessions, OVERWORLD, 0, true);
        assert!(follow(&mut sessions, OVERWORLD, 1, false).is_empty());
        assert_eq!(
            follow(&mut sessions, OVERWORLD, 1, true),
            [Begin(OVERWORLD)]
        );
    }
}
