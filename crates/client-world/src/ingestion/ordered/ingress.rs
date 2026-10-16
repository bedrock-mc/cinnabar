//! Admission credit for payloads retained by ingress lookahead.

use super::{Footprint, Held, OrderedCommitState, WorldStreamError};

impl OrderedCommitState {
    /// Records a bounded ingress payload and reserves light credit when available.
    /// False means lookahead must stop here until this payload can reserve credit.
    pub fn retain_ingress(
        &mut self,
        sequence: u64,
        footprint: Footprint,
        capacity: usize,
        retained_commits: usize,
    ) -> Result<bool, WorldStreamError> {
        self.validate_sequence(sequence)?;
        if self.ingress >= capacity {
            return Err(self.full(sequence));
        }
        self.ingress += 1;
        self.insert_entry(
            sequence,
            footprint,
            Held::Ingress {
                light_reserved: false,
            },
        );
        Ok(self.reserve_ingress_light(sequence, retained_commits))
    }

    /// Reserves a retained light payload's eventual admission before later packets pass.
    /// Heavy payloads need no light credit; false leaves a full light lane unchanged.
    pub fn reserve_ingress_light(&mut self, sequence: u64, retained_commits: usize) -> bool {
        let entry = self
            .entries
            .get(&sequence)
            .expect("the ingress reservation exists");
        let Held::Ingress { light_reserved } = entry.held else {
            panic!("only retained ingress can reserve light credit");
        };
        if entry.footprint.heavy || light_reserved {
            return true;
        }
        if self.light_capacity(retained_commits) == 0 {
            return false;
        }
        self.ingress_light += 1;
        self.entries
            .get_mut(&sequence)
            .expect("the ingress reservation exists")
            .held = Held::Ingress {
            light_reserved: true,
        };
        true
    }

    /// Borrows a retained payload's dependencies until its reservation is released.
    /// Panics if the sequence does not hold an ingress reservation.
    pub fn retained_ingress_footprint(&self, sequence: u64) -> &Footprint {
        let entry = self
            .entries
            .get(&sequence)
            .filter(|entry| matches!(entry.held, Held::Ingress { .. }))
            .expect("only retained ingress has an ingress footprint");
        &entry.footprint
    }

    /// Reports light headroom after excluding this payload's own reserved credit.
    /// Panics if the sequence does not hold an ingress reservation.
    pub fn retained_ingress_light_capacity(&self, sequence: u64, retained_commits: usize) -> usize {
        let entry = self
            .entries
            .get(&sequence)
            .expect("the ingress reservation exists");
        let Held::Ingress { light_reserved } = entry.held else {
            panic!("only retained ingress can spend its light credit");
        };
        self.light_capacity_with_reservations(
            retained_commits,
            self.ingress_light - usize::from(light_reserved),
        )
    }

    /// Releases a payload's dependency record and credit immediately before normal admission.
    pub fn release_ingress(&mut self, sequence: u64) {
        assert!(
            self.entries
                .get(&sequence)
                .is_some_and(|entry| matches!(entry.held, Held::Ingress { .. })),
            "only retained ingress can release its reservation"
        );
        self.cancel_admission(sequence);
    }
}
