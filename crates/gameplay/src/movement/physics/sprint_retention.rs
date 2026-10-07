use super::*;

impl LocalPhysicsController {
    /// Vanilla skips the sprint stop while the preceding swimming flag and this
    /// tick's body-water sensing are set.
    pub(crate) fn retains_swim_sprint(
        &self,
        world: &impl CollisionWorld,
    ) -> Result<bool, sim::WorldQueryError> {
        let Some(state) = self.state.as_ref() else {
            return Ok(false);
        };
        if self.modes.mode() != sim::MovementMode::Swimming {
            return Ok(false);
        }
        Ok(self
            .simulator
            .movement_environment(state.position, self.modes.mode(), false, world)?
            .value
            .in_water)
    }
}
