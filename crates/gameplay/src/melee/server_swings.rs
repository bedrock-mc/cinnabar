//! Local server-requested swings retain their source until transport accepts the echo.

use super::SwingTracker;
use crate::{BatchSendError, movement::MiningEffects};
use protocol::SwingSource;
use simulation_clock::identity::{PositionAuthorityGeneration, SessionGeneration};
use std::collections::VecDeque;

/// Local player state immediately before the next simulation tick.
pub struct ServerSwingContext {
    pub tick: u64,
    pub runtime_id: u64,
    pub effects: MiningEffects,
    pub item_duration: Option<u32>,
}

/// Ordered server callbacks waiting for the local swing guard and outbound FIFO.
#[derive(Default)]
pub struct ServerSwingEcho {
    authority: Option<(SessionGeneration, PositionAuthorityGeneration)>,
    pending: VecDeque<SwingSource>,
}

impl ServerSwingEcho {
    /// Discards callbacks belonging to a previous local player or dimension.
    pub fn synchronize(&mut self, authority: (SessionGeneration, PositionAuthorityGeneration)) {
        if self.authority != Some(authority) {
            self.pending.clear();
            self.authority = Some(authority);
        }
    }

    /// Appends callbacks without merging repeated sources in the same tick.
    pub fn extend(&mut self, sources: impl IntoIterator<Item = SwingSource>) {
        self.pending.extend(sources);
    }

    /// Sends admitted callbacks in order and retains an unaccepted callback on backpressure.
    pub fn flush(
        &mut self,
        context: ServerSwingContext,
        swings: &mut SwingTracker,
        mut send: impl FnMut(protocol::Packet) -> Result<(), BatchSendError>,
    ) -> Result<(), BatchSendError> {
        while let Some(source) = self.pending.front().copied() {
            let mut candidate = swings.clone();
            if candidate.try_server_swing_before_tick_for_item(
                context.tick,
                context.effects,
                context.item_duration,
            ) {
                send(protocol::swing_arm_packet(context.runtime_id, source))?;
            }
            *swings = candidate;
            self.pending.pop_front();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Provides an unchanged six-tick swing before the caller's requested tick.
    fn context(tick: u64) -> ServerSwingContext {
        ServerSwingContext {
            tick,
            runtime_id: 42,
            effects: MiningEffects::default(),
            item_duration: None,
        }
    }

    #[test]
    fn server_callbacks_echo_each_admitted_source_with_exact_wire_fields() {
        let mut echo = ServerSwingEcho::default();
        let mut swings = SwingTracker::default();
        echo.extend([SwingSource::None, SwingSource::Attack, SwingSource::Event]);
        let mut received = Vec::new();
        echo.flush(context(1), &mut swings, |packet| {
            let mut decoded = protocol::decode_batch(protocol::encode(&packet).unwrap()).unwrap();
            assert_eq!(decoded.len(), 1);
            let protocol::WorldEvent::ItemActor(protocol::ItemActorEvent::Action(action)) =
                protocol::into_world_event(decoded.remove(0), 0)
                    .unwrap()
                    .unwrap()
            else {
                panic!("expected a swing packet");
            };
            assert_eq!(action.actor_runtime_ids.as_ref(), &[42]);
            assert_eq!(action.kind, protocol::ActorActionKind::SwingArm);
            assert_eq!(action.data, 0.0);
            received.push(action.swing_source.unwrap());
            Ok(())
        })
        .unwrap();
        assert_eq!(
            received
                .iter()
                .map(|source| source.as_ref())
                .collect::<Vec<_>>(),
            ["none", "attack", "event"]
        );
        echo.extend([SwingSource::Mine]);
        echo.flush(context(2), &mut swings, |_| {
            panic!("the active first half rejects the callback")
        })
        .unwrap();
        echo.extend([SwingSource::Mine]);
        let mut count = 0;
        echo.flush(context(5), &mut swings, |_| {
            count += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn full_transport_retains_the_callback_without_starting_a_swing() {
        let mut echo = ServerSwingEcho::default();
        let mut swings = SwingTracker::default();
        echo.extend([SwingSource::Attack]);
        assert_eq!(
            echo.flush(context(1), &mut swings, |_| Err(BatchSendError::Full)),
            Err(BatchSendError::Full)
        );
        let mut count = 0;
        echo.flush(context(2), &mut swings, |_| {
            count += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(count, 1);
        assert!(echo.pending.is_empty());
    }

    #[test]
    fn authority_changes_discard_callbacks_that_could_not_be_sent() {
        let mut echo = ServerSwingEcho::default();
        echo.synchronize((
            SessionGeneration::new(1),
            PositionAuthorityGeneration::new(1),
        ));
        echo.extend([SwingSource::Attack]);
        echo.synchronize((
            SessionGeneration::new(1),
            PositionAuthorityGeneration::new(2),
        ));
        echo.flush(context(1), &mut SwingTracker::default(), |_| {
            panic!("old callback survived a dimension change")
        })
        .unwrap();
    }
}
