//! Original compatibility fixtures for tests, never for runtime classification.

/// Gives a category a stable original fixture identity independent from session generations.
pub fn category(text: &str) -> client_world::UseCooldownCategory {
    let index = text.bytes().fold(0_u16, |hash, byte| {
        hash.wrapping_mul(31).wrapping_add(u16::from(byte))
    });
    client_world::UseCooldownCategory {
        generation: 0,
        index,
    }
}

/// Resolves an explicit compatibility admission before applying stack charge facts in a test.
pub fn classify(
    identifier: &str,
    charged: bool,
    quick_charge: u8,
    ticks: Option<u32>,
) -> Option<super::AirUse> {
    let profile = assets::item_use::diagnostic_profile(identifier);
    let category = match &profile {
        assets::item_use::ItemUseProfile::Provisional {
            cooldown: Some(cooldown),
            ..
        } => Some(category(&cooldown.category)),
        _ => None,
    };
    super::classify(
        client_world::resolve_item_use(&profile, ticks, category),
        charged,
        quick_charge,
    )
}
