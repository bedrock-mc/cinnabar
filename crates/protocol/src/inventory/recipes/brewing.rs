//! Potion and container mix wire records, retained independently from recipe IDs.

/// One raw item ID and auxiliary value from a potion mix, before registry admission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BrewingMixItem {
    pub network_id: i32,
    pub aux: i32,
}

/// Brewing records have neither a stack count nor an ordinary recipe network ID.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BrewingMix {
    Potion {
        input: BrewingMixItem,
        reagent: BrewingMixItem,
        output: BrewingMixItem,
    },
    Container {
        input: i32,
        reagent: i32,
        output: i32,
    },
}

impl BrewingMix {
    /// Projects the retained wire family into the screen recipe's exact or wildcard inputs.
    pub(super) fn screen_recipe(self) -> super::screen::BrewingRecipe {
        let [input, reagent, output] = match self {
            Self::Potion {
                input,
                reagent,
                output,
            } => [input, reagent, output].map(|item| super::screen::BrewingItem {
                network_id: item.network_id,
                aux: Some(item.aux),
            }),
            Self::Container {
                input,
                reagent,
                output,
            } => [input, reagent, output].map(|network_id| super::screen::BrewingItem {
                network_id,
                aux: None,
            }),
        };
        super::screen::BrewingRecipe {
            input,
            reagent,
            output,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::recipes::{budget::Credits, decode::decode_with_credits};
    use bedrock_protocol_1_26_51::{
        packets::CraftingData,
        types::{ContainerMixDataEntry, PotionMixDataEntry},
        wire::{self, Encode},
    };
    use std::mem::size_of;

    /// Wire admission preserves signed descriptor values until the registry owner resolves them.
    #[test]
    fn wire_mix_records_preserve_family_order_and_raw_aux_values() {
        let body = CraftingData {
            potion_mixes: vec![PotionMixDataEntry {
                from_potion_id: wire::ZigZag32(-2),
                from_item_aux: wire::ZigZag32(32_768),
                reagent_item_id: wire::ZigZag32(3),
                reagent_item_aux: wire::ZigZag32(-1),
                to_potion_id: wire::ZigZag32(4),
                to_item_aux: wire::ZigZag32(65_537),
            }],
            container_mixes: vec![ContainerMixDataEntry {
                from_item_id: wire::ZigZag32(5),
                reagent_item_id: wire::ZigZag32(6),
                to_item_id: wire::ZigZag32(-7),
            }],
            clear_recipes: true,
            ..Default::default()
        }
        .encode_to_vec()
        .unwrap();
        let owner = Credits::isolated(4096);
        let update = decode_with_credits(&body, &owner).unwrap();
        assert_eq!(update.record_count(), 0);
        assert!(update.clears_catalog());
        assert_eq!(
            update.brewing_mixes().unwrap(),
            [
                BrewingMix::Potion {
                    input: BrewingMixItem {
                        network_id: -2,
                        aux: 32_768
                    },
                    reagent: BrewingMixItem {
                        network_id: 3,
                        aux: -1
                    },
                    output: BrewingMixItem {
                        network_id: 4,
                        aux: 65_537
                    },
                },
                BrewingMix::Container {
                    input: 5,
                    reagent: 6,
                    output: -7
                },
            ]
        );
    }

    /// Brewing storage consumes its own lifetime credit without reserving ordinary recipe cells.
    #[test]
    fn wire_mix_storage_stays_charged_until_the_last_update_drops() {
        let owner = Credits::isolated(usize::MAX);
        let empty = decode_with_credits(&[0; 12], &owner).unwrap();
        let overhead = owner.used();
        drop(empty);
        assert_eq!(owner.used(), 0);
        let count = 64;
        let body = CraftingData {
            potion_mixes: vec![PotionMixDataEntry::default(); count],
            ..Default::default()
        }
        .encode_to_vec()
        .unwrap();
        let bytes =
            count * (size_of::<BrewingMix>() + size_of::<super::super::screen::BrewingRecipe>());
        let limited = Credits::isolated(overhead + bytes - 1);
        assert!(
            decode_with_credits(&body, &limited)
                .unwrap()
                .is_unavailable()
        );
        assert_eq!(limited.used(), 0);

        let exact = Credits::isolated(overhead + bytes);
        let update = decode_with_credits(&body, &exact).unwrap();
        assert_eq!(update.brewing_mixes().unwrap().len(), count);
        assert_eq!(exact.used(), overhead + bytes);
        let retained = update.clone();
        drop(update);
        assert_eq!(exact.used(), overhead + bytes);
        drop(retained);
        assert_eq!(exact.used(), 0);
    }
}
