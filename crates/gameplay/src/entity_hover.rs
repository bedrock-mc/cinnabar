//! Change-driven entity hover reports from local player ticks.
use simulation_clock::identity::SessionGeneration;

/// One retained primary actor pick, including the persistent identity used for change detection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HoveredActor {
    pub unique_id: i64,
    pub runtime_id: u64,
    pub point: [f32; 3],
}

/// Tick-time hover inputs; the held item key contains its numeric item ID and auxiliary value.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct HoverSnapshot {
    pub actor: Option<HoveredActor>,
    pub held_item: (i32, u16),
}

/// Remembers only reports admitted to the outbound FIFO, allowing failed sends to retry.
#[derive(Debug, Default)]
pub struct EntityHover {
    session: Option<SessionGeneration>,
    last_tick: Option<u64>,
    reported: Option<(Option<i64>, (i32, u16))>,
}

impl EntityHover {
    /// Reports actor or held-item changes once per local tick; point-only changes stay silent.
    pub fn send_tick<E>(
        &mut self,
        session: SessionGeneration,
        tick: u64,
        snapshot: HoverSnapshot,
        mut send: impl FnMut(protocol::Packet) -> Result<(), E>,
    ) -> Result<(), E> {
        if self.session != Some(session) {
            *self = Self {
                session: Some(session),
                ..Self::default()
            };
        }
        if self.last_tick.is_some_and(|last| tick <= last) {
            return Ok(());
        }
        let key = (
            snapshot.actor.map(|actor| actor.unique_id),
            snapshot.held_item,
        );
        if self.reported != Some(key) {
            send(protocol::entity_hover_packet(
                snapshot.actor.map(|actor| (actor.runtime_id, actor.point)),
            ))?;
            self.reported = Some(key);
        }
        self.last_tick = Some(tick);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::test_support::packets::entity_hover;

    /// Supplies a retained intercept distinct from both actor-relative and eye positions.
    fn snapshot() -> HoverSnapshot {
        HoverSnapshot {
            actor: Some(HoveredActor {
                unique_id: -12,
                runtime_id: 99,
                point: [4.5, 2.6, 7.9],
            }),
            held_item: (7, 2),
        }
    }

    #[test]
    fn entity_hover_changes_only_on_actor_or_item_and_clears_without_a_point() {
        let mut owner = EntityHover::default();
        let mut packets = Vec::new();
        let session = SessionGeneration::new(3);
        let mut next = |owner: &mut EntityHover, tick, snapshot| {
            owner
                .send_tick(session, tick, snapshot, |packet| {
                    packets.push(packet);
                    Ok::<_, ()>(())
                })
                .unwrap();
        };
        let original = snapshot();
        next(&mut owner, 1, original);
        next(&mut owner, 2, original);
        let mut changed = original;
        changed.actor.as_mut().unwrap().point[0] += 0.1;
        next(&mut owner, 3, changed);
        changed.held_item = (8, 0);
        next(&mut owner, 4, changed);
        changed.actor = None;
        next(&mut owner, 5, changed);
        assert_eq!(
            packets.iter().filter_map(entity_hover).collect::<Vec<_>>(),
            [
                (99, Some(original.actor.unwrap().point)),
                (99, Some([4.6, 2.6, 7.9])),
                (0, None),
            ]
        );
    }

    #[test]
    fn entity_hover_failed_send_retries_and_a_new_session_reports_again() {
        let mut owner = EntityHover::default();
        let session = SessionGeneration::new(3);
        assert_eq!(
            owner.send_tick(session, 8, snapshot(), |_| Err("full")),
            Err("full")
        );
        let mut count = 0;
        for (session, tick) in [(session, 8), (session, 8), (SessionGeneration::new(4), 1)] {
            owner
                .send_tick(session, tick, snapshot(), |_| {
                    count += 1;
                    Ok::<_, ()>(())
                })
                .unwrap();
        }
        assert_eq!(count, 2);
    }

    #[test]
    fn entity_hover_first_empty_tick_reports_one_clear() {
        let mut owner = EntityHover::default();
        let mut packets = Vec::new();
        for tick in 1..=2 {
            owner
                .send_tick(
                    SessionGeneration::new(3),
                    tick,
                    HoverSnapshot::default(),
                    |packet| {
                        packets.push(packet);
                        Ok::<_, ()>(())
                    },
                )
                .unwrap();
        }
        assert_eq!(
            packets.iter().filter_map(entity_hover).collect::<Vec<_>>(),
            [(0, None)]
        );
    }
}
