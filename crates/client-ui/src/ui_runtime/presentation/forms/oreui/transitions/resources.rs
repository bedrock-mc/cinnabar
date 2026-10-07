//! Pack identity survives moves between the library and the staged active stack.

use super::super::motion::Tween;
use crate::global_resources::Snapshot;
use resource_pack::InstalledPack;

const REVEAL_SECONDS: f64 = 0.160;
const TRANSFER_SECONDS: f64 = 0.180;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in super::super) enum Group {
    Active,
    Available,
}

pub(in super::super) struct Card {
    pub(in super::super) pack: InstalledPack,
    pub(in super::super) group: Group,
    pub(in super::super) index: usize,
    pub(in super::super) present: bool,
    pub(in super::super) visibility: f32,
    pub(in super::super) details: f32,
    visibility_tween: Tween,
    details_tween: Tween,
}

pub(in super::super) struct Resources {
    pub(in super::super) cards: Vec<Card>,
    groups: [Tween; 2],
    pub(in super::super) expanded: [f32; 2],
    pending: Tween,
    pub(in super::super) pending_fraction: f32,
    empty: Tween,
    pub(in super::super) empty_fraction: f32,
    mounted: bool,
    touched: bool,
}

impl Default for Resources {
    fn default() -> Self {
        Self {
            cards: Vec::new(),
            groups: [Tween::at(0.0); 2],
            expanded: [0.0; 2],
            pending: Tween::at(0.0),
            pending_fraction: 0.0,
            empty: Tween::at(0.0),
            empty_fraction: 0.0,
            mounted: false,
            touched: false,
        }
    }
}

impl Resources {
    pub(in super::super) fn sync(&mut self, snapshot: &Snapshot, seconds: f64, animated: bool) {
        let first = !self.mounted;
        self.mounted = true;
        self.touched = true;
        for (index, expanded) in [snapshot.active_expanded, snapshot.available_expanded]
            .into_iter()
            .enumerate()
        {
            let target = u8::from(expanded) as f32;
            if first || !animated {
                self.groups[index] = Tween::at(target);
            }
            self.expanded[index] = self.groups[index].retarget(target, REVEAL_SECONDS, seconds);
        }
        let pending = u8::from(snapshot.has_pending_changes()) as f32;
        if first || !animated {
            self.pending = Tween::at(pending);
        }
        self.pending_fraction = self.pending.retarget(pending, REVEAL_SECONDS, seconds);
        let empty = u8::from(snapshot.available.is_empty()) as f32;
        if first || !animated {
            self.empty = Tween::at(empty);
        }
        self.empty_fraction = self.empty.retarget(empty, REVEAL_SECONDS, seconds);
        for card in &mut self.cards {
            card.present = false;
        }
        for (group, packs) in [
            (Group::Active, &snapshot.active),
            (Group::Available, &snapshot.available),
        ] {
            for (index, pack) in packs.iter().enumerate() {
                let active = group == Group::Active;
                let details = u8::from(snapshot.details_expanded == Some((active, index))) as f32;
                let card_index = self
                    .cards
                    .iter()
                    .position(|card| card.group == group && card.pack.id == pack.id)
                    .unwrap_or_else(|| {
                        self.cards.push(Card {
                            pack: pack.clone(),
                            group,
                            index,
                            present: true,
                            visibility: 1.0,
                            details,
                            visibility_tween: Tween::at(if first || !animated { 1.0 } else { 0.0 }),
                            details_tween: Tween::at(if first || !animated {
                                details
                            } else {
                                0.0
                            }),
                        });
                        self.cards.len() - 1
                    });
                let card = &mut self.cards[card_index];
                if card.pack.revision != pack.revision {
                    card.pack = pack.clone();
                }
                card.index = index;
                card.present = true;
                if !animated {
                    card.visibility_tween = Tween::at(1.0);
                    card.details_tween = Tween::at(details);
                }
                card.visibility = card
                    .visibility_tween
                    .retarget(1.0, TRANSFER_SECONDS, seconds);
                card.details = card
                    .details_tween
                    .retarget(details, REVEAL_SECONDS, seconds);
            }
        }
        for card in self.cards.iter_mut().filter(|card| !card.present) {
            if !animated {
                card.visibility_tween = Tween::at(0.0);
                card.details_tween = Tween::at(0.0);
            }
            card.visibility = card
                .visibility_tween
                .retarget(0.0, TRANSFER_SECONDS, seconds);
            card.details = card.details_tween.retarget(0.0, REVEAL_SECONDS, seconds);
        }
        self.cards
            .retain(|card| card.present || card.visibility > 0.0);
        self.cards.sort_by_key(|card| card.index);
    }

    pub(super) fn end_frame(&mut self) {
        if !std::mem::take(&mut self.touched) {
            *self = Self::default();
        }
    }
}

#[cfg(test)]
mod tests;
