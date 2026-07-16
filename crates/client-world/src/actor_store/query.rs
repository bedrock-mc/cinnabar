use super::*;

impl ActorStore {
    pub(crate) fn render_players(
        &self,
        excluded_runtime_id: Option<u64>,
    ) -> Vec<(&ActorSnapshot, Option<&PlayerProfile>)> {
        let mut players = self
            .actors
            .values()
            .filter(|actor| Some(actor.runtime_id) != excluded_runtime_id)
            .filter_map(|actor| {
                let ActorKind::Player { uuid, .. } = &actor.kind else {
                    return None;
                };
                let profile = self
                    .players
                    .get(uuid)
                    .filter(|profile| profile.unique_id == actor.unique_id);
                Some((actor, profile))
            })
            .collect::<Vec<_>>();
        players.sort_unstable_by_key(|(actor, _)| actor.runtime_id);
        players
    }
    pub(crate) fn get(&self, runtime_id: u64) -> Option<&ActorSnapshot> {
        self.actors.get(&runtime_id)
    }
    pub(crate) fn len(&self) -> usize {
        self.actors.len()
    }
    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.actors.is_empty()
    }
    #[cfg(test)]
    pub(crate) fn player_count(&self) -> usize {
        self.players.len()
    }
}
