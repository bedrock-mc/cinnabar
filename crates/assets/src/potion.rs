//! Potion appearance shared with the reviewed item sprite routes.

use std::sync::LazyLock;

/// Effect names in the native projectile texture array.
#[derive(Clone, Copy)]
enum Appearance {
    Slowness = 0,
    Speed = 1,
    MiningFatigue = 2,
    Haste = 3,
    Strength = 4,
    Healing = 5,
    Harming = 6,
    JumpBoost = 7,
    Nausea = 8,
    Regeneration = 9,
    Resistance = 10,
    FireResistance = 11,
    WaterBreathing = 12,
    Invisibility = 13,
    Blindness = 14,
    NightVision = 15,
    Hunger = 16,
    Weakness = 17,
    Poison = 18,
    Wither = 19,
    HealthBoost = 20,
    Absorption = 21,
    Saturation = 22,
    TurtleMaster = 24,
    SlowFalling = 25,
    WindCharged = 26,
    Weaving = 27,
    Oozing = 28,
    Infested = 29,
    Default = 30,
}

// The item atlas has its own ordering and omits the projectile's levitation slot.
const ATLAS_APPEARANCES: [Appearance; 30] = {
    use Appearance::*;
    [
        Default,
        Speed,
        Slowness,
        Haste,
        MiningFatigue,
        Strength,
        Healing,
        Harming,
        JumpBoost,
        Nausea,
        Regeneration,
        Resistance,
        FireResistance,
        WaterBreathing,
        Invisibility,
        Blindness,
        NightVision,
        Hunger,
        Weakness,
        Poison,
        Wither,
        HealthBoost,
        Absorption,
        Saturation,
        TurtleMaster,
        SlowFalling,
        WindCharged,
        Weaving,
        Oozing,
        Infested,
    ]
};

static APPEARANCES: LazyLock<[Option<Appearance>; 64]> = LazyLock::new(|| {
    let mut appearances = [None; 64];
    for line in include_str!("../data/legacy-icon-routes-26.30.tsv").lines() {
        let mut columns = line.split('\t');
        if columns.next() != Some("minecraft:splash_potion") {
            continue;
        }
        let metadata: usize = columns.next().unwrap().parse().unwrap();
        assert_eq!(columns.next(), Some("potion_bottle_splash"));
        let atlas: usize = columns.next().unwrap().parse().unwrap();
        assert!(
            appearances[metadata].is_none(),
            "duplicate potion appearance route"
        );
        appearances[metadata] = Some(ATLAS_APPEARANCES[atlas]);
    }
    appearances
});

/// Returns the projectile effect index for a known potion auxiliary value.
/// Unknown values have no reviewed item route and return `None`.
pub fn vanilla_potion_variant(auxiliary: i16) -> Option<u8> {
    usize::try_from(auxiliary)
        .ok()
        .and_then(|index| APPEARANCES.get(index).copied().flatten())
        .map(|appearance| appearance as u8)
}
