//! Fixed-step time reads and bounded scheduled callback staging.
use super::*;
use crate::limits::{MAX_SCHEDULE_TICKS, MAX_SCHEDULED_UPDATES};
use crate::protocol::{ScheduleHandle, SimulationStep};

impl From<SimulationStep> for wit::SimulationStep {
    fn from(value: SimulationStep) -> Self {
        Self {
            tick: value.tick,
            duration_nanos: value.duration_nanos,
        }
    }
}
impl From<ScheduleHandle> for wit::ScheduleHandle {
    fn from(value: ScheduleHandle) -> Self {
        Self {
            world: value.world.into(),
            sequence: value.sequence,
            slot: value.slot,
        }
    }
}
impl From<wit::ScheduleHandle> for ScheduleHandle {
    fn from(value: wit::ScheduleHandle) -> Self {
        Self {
            world: value.world.into(),
            sequence: value.sequence,
            slot: value.slot,
        }
    }
}
impl CallbackRes {
    /// Reads the instance counter supplied by the adapter, without ambient time.
    pub(crate) fn simulation(&mut self) -> Result<Result<wit::SimulationStep, WorldError>> {
        self.host_call()?;
        Ok(self
            .simulation
            .as_ref()
            .map(|s| s.step.into())
            .ok_or(WorldError::Unavailable))
    }
    /// Stages a timer relative to commit, subject to the extension's scheduling quota.
    pub(crate) fn schedule(
        &mut self,
        after: u32,
        period: u32,
    ) -> Result<Result<wit::ScheduleHandle, WorldError>> {
        self.host_call()?;
        let (Some(sim), Some(context)) = (&mut self.simulation, &self.context) else {
            return Ok(Err(WorldError::Unavailable));
        };
        if after == 0 || after > MAX_SCHEDULE_TICKS || period > MAX_SCHEDULE_TICKS {
            return Ok(Err(WorldError::OutOfBounds));
        }
        if sim.schedules.len() >= MAX_SCHEDULED_UPDATES {
            return Ok(Err(WorldError::QuotaExceeded));
        }
        let handle = ScheduleHandle {
            world: context.current.clone(),
            sequence: sim.sequence,
            slot: self
                .ops
                .iter()
                .filter(|op| matches!(op, Op::Schedule { .. }))
                .count() as u32,
        };
        stage(
            &mut self.ops,
            Op::Schedule {
                handle: handle.clone(),
                after,
                period,
            },
        )?;
        sim.schedules.push(handle.clone());
        Ok(Ok(handle.into()))
    }
    /// Cancels only an active handle visible in this callback's instance snapshot.
    pub(crate) fn cancel_schedule(
        &mut self,
        handle: wit::ScheduleHandle,
    ) -> Result<Result<(), WorldError>> {
        self.host_call()?;
        let handle: ScheduleHandle = handle.into();
        let Some(sim) = &mut self.simulation else {
            return Ok(Err(WorldError::Unavailable));
        };
        let Some(index) = sim.schedules.iter().position(|h| h == &handle) else {
            return Ok(Err(WorldError::Unavailable));
        };
        stage(&mut self.ops, Op::CancelSchedule { handle })?;
        sim.schedules.remove(index);
        Ok(Ok(()))
    }
}
