//! Consumption presentation selected from effective item-use admissions.

use assets::item_use::ItemConsumption;

/// How a consumable is used: eaten food scatters crumbs, drinks do not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsumeAnimation {
    Eat,
    Drink,
}

/// Selects the use animation from the item's admitted consumption behavior.
#[must_use]
pub fn consume_animation(consumption: ItemConsumption) -> Option<ConsumeAnimation> {
    match consumption {
        ItemConsumption::Eat => Some(ConsumeAnimation::Eat),
        ItemConsumption::Drink | ItemConsumption::Honey => Some(ConsumeAnimation::Drink),
        ItemConsumption::Unresolved | ItemConsumption::None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn honey_is_drunk_without_food_crumbs() {
        assert_eq!(
            consume_animation(ItemConsumption::Honey),
            Some(ConsumeAnimation::Drink)
        );
    }
}
