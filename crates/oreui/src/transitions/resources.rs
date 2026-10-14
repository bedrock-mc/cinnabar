//! Card identity survives moves between a library and its active group.

use super::super::motion::{Surface, Tween};
use launcher::global_resources::Snapshot;
use launcher::mods::InstalledMod;
use resource_pack::InstalledPack;

const REVEAL_SECONDS: f64 = 0.160;
const TRANSFER_SECONDS: f64 = 0.180;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in super::super) enum Group {
    Active,
    Available,
}

/// A card's payload: `same` follows it across groups and `changed` refreshes its retained copy.
pub(in super::super) trait CardItem: Clone {
    fn same(&self, other: &Self) -> bool;
    fn changed(&self, other: &Self) -> bool;
    fn surface(&self, active: bool) -> Surface;
}

impl CardItem for InstalledPack {
    fn same(&self, other: &Self) -> bool {
        self.id == other.id
    }

    fn changed(&self, other: &Self) -> bool {
        self.revision != other.revision
    }

    fn surface(&self, active: bool) -> Surface {
        Surface::ResourcePack(self.id.as_u128(), active)
    }
}

impl CardItem for InstalledMod {
    fn same(&self, other: &Self) -> bool {
        self.id == other.id
    }

    fn changed(&self, other: &Self) -> bool {
        self != other
    }

    fn surface(&self, active: bool) -> Surface {
        Surface::Mod(self.id, active)
    }
}

/// The lists and disclosures one frame shows.
pub(in super::super) struct Listing<'a, T> {
    pub(in super::super) active: &'a [T],
    pub(in super::super) available: &'a [T],
    /// Active then available group disclosure.
    pub(in super::super) expanded: [bool; 2],
    pub(in super::super) details: Option<(bool, usize)>,
    pub(in super::super) pending: bool,
}

pub(in super::super) struct Card<T> {
    pub(in super::super) item: T,
    pub(in super::super) group: Group,
    pub(in super::super) index: usize,
    pub(in super::super) present: bool,
    pub(in super::super) visibility: f32,
    pub(in super::super) details: f32,
    visibility_tween: Tween,
    details_tween: Tween,
}

pub(in super::super) struct Cards<T> {
    pub(in super::super) cards: Vec<Card<T>>,
    groups: [Tween; 2],
    pub(in super::super) expanded: [f32; 2],
    pending: Tween,
    pub(in super::super) pending_fraction: f32,
    empty: Tween,
    pub(in super::super) empty_fraction: f32,
    mounted: bool,
    touched: bool,
}

/// Global resource pack cards.
pub(in super::super) type Resources = Cards<InstalledPack>;
/// Settings Mods tab cards.
pub(in super::super) type Mods = Cards<InstalledMod>;

impl<T> Default for Cards<T> {
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
        self.sync_listing(
            Listing {
                active: &snapshot.active,
                available: &snapshot.available,
                expanded: [snapshot.active_expanded, snapshot.available_expanded],
                details: snapshot.details_expanded,
                pending: snapshot.has_pending_changes(),
            },
            seconds,
            animated,
        );
    }
}

impl<T: CardItem> Cards<T> {
    /// Card transfers and disclosures must finish before their frame is retained.
    pub fn active(&self, seconds: f64) -> bool {
        self.groups.iter().any(|t| t.active(seconds))
            || self.pending.active(seconds)
            || self.empty.active(seconds)
            || self
                .cards
                .iter()
                .any(|c| c.visibility_tween.active(seconds) || c.details_tween.active(seconds))
    }

    pub(in super::super) fn sync_listing(
        &mut self,
        listing: Listing<'_, T>,
        seconds: f64,
        animated: bool,
    ) {
        let first = !self.mounted;
        self.mounted = true;
        self.touched = true;
        for (index, expanded) in listing.expanded.into_iter().enumerate() {
            let target = u8::from(expanded) as f32;
            if first || !animated {
                self.groups[index] = Tween::at(target);
            }
            self.expanded[index] = self.groups[index].retarget(target, REVEAL_SECONDS, seconds);
        }
        let pending = u8::from(listing.pending) as f32;
        if first || !animated {
            self.pending = Tween::at(pending);
        }
        self.pending_fraction = self.pending.retarget(pending, REVEAL_SECONDS, seconds);
        let empty = u8::from(listing.available.is_empty()) as f32;
        if first || !animated {
            self.empty = Tween::at(empty);
        }
        self.empty_fraction = self.empty.retarget(empty, REVEAL_SECONDS, seconds);
        for card in &mut self.cards {
            card.present = false;
        }
        for (group, items) in [
            (Group::Active, listing.active),
            (Group::Available, listing.available),
        ] {
            for (index, item) in items.iter().enumerate() {
                let active = group == Group::Active;
                let details = u8::from(listing.details == Some((active, index))) as f32;
                let card_index = self
                    .cards
                    .iter()
                    .position(|card| card.group == group && card.item.same(item))
                    .unwrap_or_else(|| {
                        self.cards.push(Card {
                            item: item.clone(),
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
                if card.item.changed(item) {
                    card.item = item.clone();
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
