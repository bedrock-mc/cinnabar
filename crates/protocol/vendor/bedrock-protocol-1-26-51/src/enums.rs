// Code generated from canonical protocol manifest v2. DO NOT EDIT.

use crate::wire;

// Domain: actor

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ActorEventType {
    #[default]
    None,
    Jump,
    Hurt,
    Death,
    StartAttacking,
    StopAttacking,
    TamingFailed,
    TamingSucceeded,
    ShakeWetness,
    EatGrass,
    FishHookBubble,
    FishHookFishPos,
    FishHookHookTime,
    FishHookTease,
    SquidFleeing,
    ZombieConverting,
    PlayAmbient,
    SpawnAlive,
    StartOfferFlower,
    StopOfferFlower,
    LoveHearts,
    VillagerAngry,
    VillagerHappy,
    WitchHatMagic,
    FireworksExplode,
    InLoveHearts,
    SilverfishMergeAnim,
    GuardianAttackSound,
    DrinkPotion,
    ThrowPotion,
    PrimeTntCart,
    PrimeCreeper,
    AirSupply,
    DeprecatedAddPlayerLevels,
    GuardianMiningFatigue,
    AgentSwingArm,
    DragonStartDeathAnim,
    GroundDust,
    Shake,
    Feed,
    BabyAge,
    InstantDeath,
    NotifyTrade,
    LeashDestroyed,
    CaravanUpdated,
    TalismanActivate,
    DeprecatedUpdateStructureFeature,
    PlayerSpawnedMob,
    Puke,
    UpdateStackSize,
    StartSwimming,
    BalloonPop,
    TreasureHunt,
    SummonAgent,
    FinishedChargingItem,
    ActorGrowUp,
    VibrationDetected,
    DrinkMilk,
    ShakeWetnessStop,
    KineticDamageDealt,
    HurtWithoutReceivingDamage,
    Unknown(u8),
}

impl From<u8> for ActorEventType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Jump,
            2 => Self::Hurt,
            3 => Self::Death,
            4 => Self::StartAttacking,
            5 => Self::StopAttacking,
            6 => Self::TamingFailed,
            7 => Self::TamingSucceeded,
            8 => Self::ShakeWetness,
            10 => Self::EatGrass,
            11 => Self::FishHookBubble,
            12 => Self::FishHookFishPos,
            13 => Self::FishHookHookTime,
            14 => Self::FishHookTease,
            15 => Self::SquidFleeing,
            16 => Self::ZombieConverting,
            17 => Self::PlayAmbient,
            18 => Self::SpawnAlive,
            19 => Self::StartOfferFlower,
            20 => Self::StopOfferFlower,
            21 => Self::LoveHearts,
            22 => Self::VillagerAngry,
            23 => Self::VillagerHappy,
            24 => Self::WitchHatMagic,
            25 => Self::FireworksExplode,
            26 => Self::InLoveHearts,
            27 => Self::SilverfishMergeAnim,
            28 => Self::GuardianAttackSound,
            29 => Self::DrinkPotion,
            30 => Self::ThrowPotion,
            31 => Self::PrimeTntCart,
            32 => Self::PrimeCreeper,
            33 => Self::AirSupply,
            34 => Self::DeprecatedAddPlayerLevels,
            35 => Self::GuardianMiningFatigue,
            36 => Self::AgentSwingArm,
            37 => Self::DragonStartDeathAnim,
            38 => Self::GroundDust,
            39 => Self::Shake,
            57 => Self::Feed,
            60 => Self::BabyAge,
            61 => Self::InstantDeath,
            62 => Self::NotifyTrade,
            63 => Self::LeashDestroyed,
            64 => Self::CaravanUpdated,
            65 => Self::TalismanActivate,
            66 => Self::DeprecatedUpdateStructureFeature,
            67 => Self::PlayerSpawnedMob,
            68 => Self::Puke,
            69 => Self::UpdateStackSize,
            70 => Self::StartSwimming,
            71 => Self::BalloonPop,
            72 => Self::TreasureHunt,
            73 => Self::SummonAgent,
            74 => Self::FinishedChargingItem,
            76 => Self::ActorGrowUp,
            77 => Self::VibrationDetected,
            78 => Self::DrinkMilk,
            79 => Self::ShakeWetnessStop,
            80 => Self::KineticDamageDealt,
            81 => Self::HurtWithoutReceivingDamage,
            value => Self::Unknown(value),
        }
    }
}

impl ActorEventType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Jump => 1,
            Self::Hurt => 2,
            Self::Death => 3,
            Self::StartAttacking => 4,
            Self::StopAttacking => 5,
            Self::TamingFailed => 6,
            Self::TamingSucceeded => 7,
            Self::ShakeWetness => 8,
            Self::EatGrass => 10,
            Self::FishHookBubble => 11,
            Self::FishHookFishPos => 12,
            Self::FishHookHookTime => 13,
            Self::FishHookTease => 14,
            Self::SquidFleeing => 15,
            Self::ZombieConverting => 16,
            Self::PlayAmbient => 17,
            Self::SpawnAlive => 18,
            Self::StartOfferFlower => 19,
            Self::StopOfferFlower => 20,
            Self::LoveHearts => 21,
            Self::VillagerAngry => 22,
            Self::VillagerHappy => 23,
            Self::WitchHatMagic => 24,
            Self::FireworksExplode => 25,
            Self::InLoveHearts => 26,
            Self::SilverfishMergeAnim => 27,
            Self::GuardianAttackSound => 28,
            Self::DrinkPotion => 29,
            Self::ThrowPotion => 30,
            Self::PrimeTntCart => 31,
            Self::PrimeCreeper => 32,
            Self::AirSupply => 33,
            Self::DeprecatedAddPlayerLevels => 34,
            Self::GuardianMiningFatigue => 35,
            Self::AgentSwingArm => 36,
            Self::DragonStartDeathAnim => 37,
            Self::GroundDust => 38,
            Self::Shake => 39,
            Self::Feed => 57,
            Self::BabyAge => 60,
            Self::InstantDeath => 61,
            Self::NotifyTrade => 62,
            Self::LeashDestroyed => 63,
            Self::CaravanUpdated => 64,
            Self::TalismanActivate => 65,
            Self::DeprecatedUpdateStructureFeature => 66,
            Self::PlayerSpawnedMob => 67,
            Self::Puke => 68,
            Self::UpdateStackSize => 69,
            Self::StartSwimming => 70,
            Self::BalloonPop => 71,
            Self::TreasureHunt => 72,
            Self::SummonAgent => 73,
            Self::FinishedChargingItem => 74,
            Self::ActorGrowUp => 76,
            Self::VibrationDetected => 77,
            Self::DrinkMilk => 78,
            Self::ShakeWetnessStop => 79,
            Self::KineticDamageDealt => 80,
            Self::HurtWithoutReceivingDamage => 81,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ActorEventType> for u8 {
    fn from(value: ActorEventType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ActorEventType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ActorEventType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ActorLinkType {
    #[default]
    None,
    Riding,
    Passenger,
    Unknown(u8),
}

impl From<u8> for ActorLinkType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Riding,
            2 => Self::Passenger,
            value => Self::Unknown(value),
        }
    }
}

impl ActorLinkType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Riding => 1,
            Self::Passenger => 2,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ActorLinkType> for u8 {
    fn from(value: ActorLinkType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ActorLinkType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ActorLinkType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ActorType {
    #[default]
    Undefined,
    Itementity,
    Primedtnt,
    Fallingblock,
    Movingblock,
    Experience,
    Eyeofender,
    Endercrystal,
    Fireworksrocket,
    Fishinghook,
    Chalkboard,
    Painting,
    Leashknot,
    Boatrideable,
    Lightningbolt,
    Areaeffectcloud,
    Balloon,
    Shield,
    Lectern,
    Ominousitemspawner,
    Cushion,
    Chestboatrideable,
    Mob,
    Npc,
    Agent,
    Armorstand,
    Tripodcamera,
    Player,
    Bee,
    Piglin,
    Piglinbrute,
    Allay,
    Pathfindermob,
    Irongolem,
    Snowgolem,
    Wanderingtrader,
    Coppergolem,
    Sulfurcube,
    Monster,
    Creeper,
    Slime,
    Enderman,
    Ghast,
    Lavaslime,
    Blaze,
    Witch,
    Guardian,
    Elderguardian,
    Dragon,
    Shulker,
    Vindicator,
    Illagerbeast,
    Evocationillager,
    Vex,
    Pillager,
    Elderguardianghost,
    Warden,
    Breeze,
    Creaking,
    Animal,
    Chicken,
    Cow,
    Pig,
    Sheep,
    Mushroomcow,
    Rabbit,
    Polarbear,
    Llama,
    Turtle,
    Panda,
    Fox,
    Hoglin,
    Strider,
    Goat,
    Axolotl,
    Frog,
    Camel,
    Sniffer,
    Armadillo,
    Happyghast,
    Traderllama,
    Wateranimal,
    Squid,
    Dolphin,
    Pufferfish,
    Salmon,
    Tropicalfish,
    Fish,
    Glowsquid,
    Tadpole,
    Nautilus,
    Tamableanimal,
    Wolf,
    Ocelot,
    Parrot,
    Cat,
    Ambient,
    Bat,
    Undeadmonster,
    Pigzombie,
    Witherboss,
    Phantom,
    Zoglin,
    Camelhusk,
    Zombienautilus,
    Zombiemonster,
    Zombie,
    Zombievillager,
    Husk,
    Drowned,
    Zombievillagerv2,
    Arthropod,
    Spider,
    Silverfish,
    Cavespider,
    Endermite,
    Minecart,
    Minecartrideable,
    Minecarthopper,
    Minecarttnt,
    Minecartchest,
    Minecartfurnace,
    Minecartcommandblock,
    Skeletonmonster,
    Skeleton,
    Stray,
    Witherskeleton,
    Bogged,
    Parched,
    Equineanimal,
    Horse,
    Donkey,
    Mule,
    Skeletonhorse,
    Zombiehorse,
    Projectile,
    Experiencepotion,
    Shulkerbullet,
    Dragonfireball,
    Snowball,
    Thrownegg,
    Largefireball,
    Thrownpotion,
    Enderpearl,
    Witherskull,
    Witherskulldangerous,
    Smallfireball,
    Lingeringpotion,
    Llamaspit,
    Evocationfang,
    Icebomb,
    Breezewindchargeprojectile,
    Windchargeprojectile,
    Abstractarrow,
    Trident,
    Arrow,
    Villagerbase,
    Villager,
    Villagerv2,
    Unknown(i32),
}

impl From<i32> for ActorType {
    fn from(value: i32) -> Self {
        match value {
            1 => Self::Undefined,
            64 => Self::Itementity,
            65 => Self::Primedtnt,
            66 => Self::Fallingblock,
            67 => Self::Movingblock,
            69 => Self::Experience,
            70 => Self::Eyeofender,
            71 => Self::Endercrystal,
            72 => Self::Fireworksrocket,
            77 => Self::Fishinghook,
            78 => Self::Chalkboard,
            83 => Self::Painting,
            88 => Self::Leashknot,
            90 => Self::Boatrideable,
            93 => Self::Lightningbolt,
            95 => Self::Areaeffectcloud,
            107 => Self::Balloon,
            117 => Self::Shield,
            119 => Self::Lectern,
            145 => Self::Ominousitemspawner,
            154 => Self::Cushion,
            218 => Self::Chestboatrideable,
            256 => Self::Mob,
            307 => Self::Npc,
            312 => Self::Agent,
            317 => Self::Armorstand,
            318 => Self::Tripodcamera,
            319 => Self::Player,
            378 => Self::Bee,
            379 => Self::Piglin,
            383 => Self::Piglinbrute,
            390 => Self::Allay,
            768 => Self::Pathfindermob,
            788 => Self::Irongolem,
            789 => Self::Snowgolem,
            886 => Self::Wanderingtrader,
            916 => Self::Coppergolem,
            921 => Self::Sulfurcube,
            2816 => Self::Monster,
            2849 => Self::Creeper,
            2853 => Self::Slime,
            2854 => Self::Enderman,
            2857 => Self::Ghast,
            2858 => Self::Lavaslime,
            2859 => Self::Blaze,
            2861 => Self::Witch,
            2865 => Self::Guardian,
            2866 => Self::Elderguardian,
            2869 => Self::Dragon,
            2870 => Self::Shulker,
            2873 => Self::Vindicator,
            2875 => Self::Illagerbeast,
            2920 => Self::Evocationillager,
            2921 => Self::Vex,
            2930 => Self::Pillager,
            2936 => Self::Elderguardianghost,
            2947 => Self::Warden,
            2956 => Self::Breeze,
            2962 => Self::Creaking,
            4864 => Self::Animal,
            4874 => Self::Chicken,
            4875 => Self::Cow,
            4876 => Self::Pig,
            4877 => Self::Sheep,
            4880 => Self::Mushroomcow,
            4882 => Self::Rabbit,
            4892 => Self::Polarbear,
            4893 => Self::Llama,
            4938 => Self::Turtle,
            4977 => Self::Panda,
            4985 => Self::Fox,
            4988 => Self::Hoglin,
            4989 => Self::Strider,
            4992 => Self::Goat,
            4994 => Self::Axolotl,
            4996 => Self::Frog,
            5002 => Self::Camel,
            5003 => Self::Sniffer,
            5006 => Self::Armadillo,
            5011 => Self::Happyghast,
            5021 => Self::Traderllama,
            8960 => Self::Wateranimal,
            8977 => Self::Squid,
            8991 => Self::Dolphin,
            9068 => Self::Pufferfish,
            9069 => Self::Salmon,
            9071 => Self::Tropicalfish,
            9072 => Self::Fish,
            9089 => Self::Glowsquid,
            9093 => Self::Tadpole,
            9109 => Self::Nautilus,
            21248 => Self::Tamableanimal,
            21262 => Self::Wolf,
            21270 => Self::Ocelot,
            21278 => Self::Parrot,
            21323 => Self::Cat,
            33024 => Self::Ambient,
            33043 => Self::Bat,
            68352 => Self::Undeadmonster,
            68388 => Self::Pigzombie,
            68404 => Self::Witherboss,
            68410 => Self::Phantom,
            68478 => Self::Zoglin,
            70552 => Self::Camelhusk,
            74646 => Self::Zombienautilus,
            199424 => Self::Zombiemonster,
            199456 => Self::Zombie,
            199468 => Self::Zombievillager,
            199471 => Self::Husk,
            199534 => Self::Drowned,
            199540 => Self::Zombievillagerv2,
            264960 => Self::Arthropod,
            264995 => Self::Spider,
            264999 => Self::Silverfish,
            265000 => Self::Cavespider,
            265015 => Self::Endermite,
            524288 => Self::Minecart,
            524372 => Self::Minecartrideable,
            524384 => Self::Minecarthopper,
            524385 => Self::Minecarttnt,
            524386 => Self::Minecartchest,
            524387 => Self::Minecartfurnace,
            524388 => Self::Minecartcommandblock,
            1116928 => Self::Skeletonmonster,
            1116962 => Self::Skeleton,
            1116974 => Self::Stray,
            1116976 => Self::Witherskeleton,
            1117072 => Self::Bogged,
            1117079 => Self::Parched,
            2118400 => Self::Equineanimal,
            2118423 => Self::Horse,
            2118424 => Self::Donkey,
            2118425 => Self::Mule,
            2183962 => Self::Skeletonhorse,
            2183963 => Self::Zombiehorse,
            4194304 => Self::Projectile,
            4194372 => Self::Experiencepotion,
            4194380 => Self::Shulkerbullet,
            4194383 => Self::Dragonfireball,
            4194385 => Self::Snowball,
            4194386 => Self::Thrownegg,
            4194389 => Self::Largefireball,
            4194390 => Self::Thrownpotion,
            4194391 => Self::Enderpearl,
            4194393 => Self::Witherskull,
            4194395 => Self::Witherskulldangerous,
            4194398 => Self::Smallfireball,
            4194405 => Self::Lingeringpotion,
            4194406 => Self::Llamaspit,
            4194407 => Self::Evocationfang,
            4194410 => Self::Icebomb,
            4194445 => Self::Breezewindchargeprojectile,
            4194447 => Self::Windchargeprojectile,
            8388608 => Self::Abstractarrow,
            12582985 => Self::Trident,
            12582992 => Self::Arrow,
            16777984 => Self::Villagerbase,
            16777999 => Self::Villager,
            16778099 => Self::Villagerv2,
            value => Self::Unknown(value),
        }
    }
}

impl ActorType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Undefined => 1,
            Self::Itementity => 64,
            Self::Primedtnt => 65,
            Self::Fallingblock => 66,
            Self::Movingblock => 67,
            Self::Experience => 69,
            Self::Eyeofender => 70,
            Self::Endercrystal => 71,
            Self::Fireworksrocket => 72,
            Self::Fishinghook => 77,
            Self::Chalkboard => 78,
            Self::Painting => 83,
            Self::Leashknot => 88,
            Self::Boatrideable => 90,
            Self::Lightningbolt => 93,
            Self::Areaeffectcloud => 95,
            Self::Balloon => 107,
            Self::Shield => 117,
            Self::Lectern => 119,
            Self::Ominousitemspawner => 145,
            Self::Cushion => 154,
            Self::Chestboatrideable => 218,
            Self::Mob => 256,
            Self::Npc => 307,
            Self::Agent => 312,
            Self::Armorstand => 317,
            Self::Tripodcamera => 318,
            Self::Player => 319,
            Self::Bee => 378,
            Self::Piglin => 379,
            Self::Piglinbrute => 383,
            Self::Allay => 390,
            Self::Pathfindermob => 768,
            Self::Irongolem => 788,
            Self::Snowgolem => 789,
            Self::Wanderingtrader => 886,
            Self::Coppergolem => 916,
            Self::Sulfurcube => 921,
            Self::Monster => 2816,
            Self::Creeper => 2849,
            Self::Slime => 2853,
            Self::Enderman => 2854,
            Self::Ghast => 2857,
            Self::Lavaslime => 2858,
            Self::Blaze => 2859,
            Self::Witch => 2861,
            Self::Guardian => 2865,
            Self::Elderguardian => 2866,
            Self::Dragon => 2869,
            Self::Shulker => 2870,
            Self::Vindicator => 2873,
            Self::Illagerbeast => 2875,
            Self::Evocationillager => 2920,
            Self::Vex => 2921,
            Self::Pillager => 2930,
            Self::Elderguardianghost => 2936,
            Self::Warden => 2947,
            Self::Breeze => 2956,
            Self::Creaking => 2962,
            Self::Animal => 4864,
            Self::Chicken => 4874,
            Self::Cow => 4875,
            Self::Pig => 4876,
            Self::Sheep => 4877,
            Self::Mushroomcow => 4880,
            Self::Rabbit => 4882,
            Self::Polarbear => 4892,
            Self::Llama => 4893,
            Self::Turtle => 4938,
            Self::Panda => 4977,
            Self::Fox => 4985,
            Self::Hoglin => 4988,
            Self::Strider => 4989,
            Self::Goat => 4992,
            Self::Axolotl => 4994,
            Self::Frog => 4996,
            Self::Camel => 5002,
            Self::Sniffer => 5003,
            Self::Armadillo => 5006,
            Self::Happyghast => 5011,
            Self::Traderllama => 5021,
            Self::Wateranimal => 8960,
            Self::Squid => 8977,
            Self::Dolphin => 8991,
            Self::Pufferfish => 9068,
            Self::Salmon => 9069,
            Self::Tropicalfish => 9071,
            Self::Fish => 9072,
            Self::Glowsquid => 9089,
            Self::Tadpole => 9093,
            Self::Nautilus => 9109,
            Self::Tamableanimal => 21248,
            Self::Wolf => 21262,
            Self::Ocelot => 21270,
            Self::Parrot => 21278,
            Self::Cat => 21323,
            Self::Ambient => 33024,
            Self::Bat => 33043,
            Self::Undeadmonster => 68352,
            Self::Pigzombie => 68388,
            Self::Witherboss => 68404,
            Self::Phantom => 68410,
            Self::Zoglin => 68478,
            Self::Camelhusk => 70552,
            Self::Zombienautilus => 74646,
            Self::Zombiemonster => 199424,
            Self::Zombie => 199456,
            Self::Zombievillager => 199468,
            Self::Husk => 199471,
            Self::Drowned => 199534,
            Self::Zombievillagerv2 => 199540,
            Self::Arthropod => 264960,
            Self::Spider => 264995,
            Self::Silverfish => 264999,
            Self::Cavespider => 265000,
            Self::Endermite => 265015,
            Self::Minecart => 524288,
            Self::Minecartrideable => 524372,
            Self::Minecarthopper => 524384,
            Self::Minecarttnt => 524385,
            Self::Minecartchest => 524386,
            Self::Minecartfurnace => 524387,
            Self::Minecartcommandblock => 524388,
            Self::Skeletonmonster => 1116928,
            Self::Skeleton => 1116962,
            Self::Stray => 1116974,
            Self::Witherskeleton => 1116976,
            Self::Bogged => 1117072,
            Self::Parched => 1117079,
            Self::Equineanimal => 2118400,
            Self::Horse => 2118423,
            Self::Donkey => 2118424,
            Self::Mule => 2118425,
            Self::Skeletonhorse => 2183962,
            Self::Zombiehorse => 2183963,
            Self::Projectile => 4194304,
            Self::Experiencepotion => 4194372,
            Self::Shulkerbullet => 4194380,
            Self::Dragonfireball => 4194383,
            Self::Snowball => 4194385,
            Self::Thrownegg => 4194386,
            Self::Largefireball => 4194389,
            Self::Thrownpotion => 4194390,
            Self::Enderpearl => 4194391,
            Self::Witherskull => 4194393,
            Self::Witherskulldangerous => 4194395,
            Self::Smallfireball => 4194398,
            Self::Lingeringpotion => 4194405,
            Self::Llamaspit => 4194406,
            Self::Evocationfang => 4194407,
            Self::Icebomb => 4194410,
            Self::Breezewindchargeprojectile => 4194445,
            Self::Windchargeprojectile => 4194447,
            Self::Abstractarrow => 8388608,
            Self::Trident => 12582985,
            Self::Arrow => 12582992,
            Self::Villagerbase => 16777984,
            Self::Villager => 16777999,
            Self::Villagerv2 => 16778099,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ActorType> for i32 {
    fn from(value: ActorType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ActorType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ActorType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

// Domain: attribute_layer

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum EASNoiseAlignmentType {
    #[default]
    Minlocaltransitionend,
    Unknown(u8),
}

impl From<u8> for EASNoiseAlignmentType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Minlocaltransitionend,
            value => Self::Unknown(value),
        }
    }
}

impl EASNoiseAlignmentType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Minlocaltransitionend => 0,
            Self::Unknown(value) => value,
        }
    }
}

impl From<EASNoiseAlignmentType> for u8 {
    fn from(value: EASNoiseAlignmentType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for EASNoiseAlignmentType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for EASNoiseAlignmentType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

// Domain: camera

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum CameraAimAssistAction {
    #[default]
    Set,
    Clear,
    Unknown(u8),
}

impl From<u8> for CameraAimAssistAction {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Set,
            1 => Self::Clear,
            value => Self::Unknown(value),
        }
    }
}

impl CameraAimAssistAction {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Set => 0,
            Self::Clear => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<CameraAimAssistAction> for u8 {
    fn from(value: CameraAimAssistAction) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for CameraAimAssistAction {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CameraAimAssistAction {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum CameraAimAssistPresetOperation {
    #[default]
    Set,
    Addtoexisting,
    Unknown(u8),
}

impl From<u8> for CameraAimAssistPresetOperation {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Set,
            1 => Self::Addtoexisting,
            value => Self::Unknown(value),
        }
    }
}

impl CameraAimAssistPresetOperation {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Set => 0,
            Self::Addtoexisting => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<CameraAimAssistPresetOperation> for u8 {
    fn from(value: CameraAimAssistPresetOperation) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for CameraAimAssistPresetOperation {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CameraAimAssistPresetOperation {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum CameraAimAssistTargetMode {
    #[default]
    Angle,
    Distance,
    Unknown(i32),
}

impl From<i32> for CameraAimAssistTargetMode {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Angle,
            1 => Self::Distance,
            value => Self::Unknown(value),
        }
    }
}

impl CameraAimAssistTargetMode {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Angle => 0,
            Self::Distance => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<CameraAimAssistTargetMode> for i32 {
    fn from(value: CameraAimAssistTargetMode) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for CameraAimAssistTargetMode {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::I32LE(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CameraAimAssistTargetMode {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::I32LE as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum CameraPresetAudioListener {
    #[default]
    Camera,
    Player,
    Unknown(u8),
}

impl From<u8> for CameraPresetAudioListener {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Camera,
            1 => Self::Player,
            value => Self::Unknown(value),
        }
    }
}

impl CameraPresetAudioListener {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Camera => 0,
            Self::Player => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<CameraPresetAudioListener> for u8 {
    fn from(value: CameraPresetAudioListener) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for CameraPresetAudioListener {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CameraPresetAudioListener {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum CameraShakeAction {
    #[default]
    Add,
    Stop,
    Unknown(u8),
}

impl From<u8> for CameraShakeAction {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Add,
            1 => Self::Stop,
            value => Self::Unknown(value),
        }
    }
}

impl CameraShakeAction {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Add => 0,
            Self::Stop => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<CameraShakeAction> for u8 {
    fn from(value: CameraShakeAction) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for CameraShakeAction {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CameraShakeAction {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum CameraShakeType {
    #[default]
    Positional,
    Rotational,
    Unknown(u8),
}

impl From<u8> for CameraShakeType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Positional,
            1 => Self::Rotational,
            value => Self::Unknown(value),
        }
    }
}

impl CameraShakeType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Positional => 0,
            Self::Rotational => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<CameraShakeType> for u8 {
    fn from(value: CameraShakeType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for CameraShakeType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CameraShakeType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

// Domain: command

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum CommandPermissionLevel {
    #[default]
    Any,
    Gamedirectors,
    Admin,
    Host,
    Owner,
    Internal,
    Unknown(u8),
}

impl From<u8> for CommandPermissionLevel {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Any,
            1 => Self::Gamedirectors,
            2 => Self::Admin,
            3 => Self::Host,
            4 => Self::Owner,
            5 => Self::Internal,
            value => Self::Unknown(value),
        }
    }
}

impl CommandPermissionLevel {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Any => 0,
            Self::Gamedirectors => 1,
            Self::Admin => 2,
            Self::Host => 3,
            Self::Owner => 4,
            Self::Internal => 5,
            Self::Unknown(value) => value,
        }
    }
}

impl From<CommandPermissionLevel> for u8 {
    fn from(value: CommandPermissionLevel) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for CommandPermissionLevel {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CommandPermissionLevel {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

// Domain: container

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ContainerEnumName {
    #[default]
    Anvilinputcontainer,
    Anvilmaterialcontainer,
    Anvilresultpreviewcontainer,
    Smithingtableinputcontainer,
    Smithingtablematerialcontainer,
    Smithingtableresultpreviewcontainer,
    Armorcontainer,
    Levelentitycontainer,
    Beaconpaymentcontainer,
    Brewingstandinputcontainer,
    Brewingstandresultcontainer,
    Brewingstandfuelcontainer,
    Combinedhotbarandinventorycontainer,
    Craftinginputcontainer,
    Craftingoutputpreviewcontainer,
    Recipeconstructioncontainer,
    Recipenaturecontainer,
    Recipeitemscontainer,
    Recipesearchcontainer,
    Recipesearchbarcontainer,
    Recipeequipmentcontainer,
    Recipebookcontainer,
    Enchantinginputcontainer,
    Enchantingmaterialcontainer,
    Furnacefuelcontainer,
    Furnaceingredientcontainer,
    Furnaceresultcontainer,
    Horseequipcontainer,
    Hotbarcontainer,
    Inventorycontainer,
    Shulkerboxcontainer,
    Tradeingredient1container,
    Tradeingredient2container,
    Traderesultpreviewcontainer,
    Offhandcontainer,
    Compoundcreatorinput,
    Compoundcreatoroutputpreview,
    Elementconstructoroutputpreview,
    Materialreducerinput,
    Materialreduceroutput,
    Labtableinput,
    Loominputcontainer,
    Loomdyecontainer,
    Loommaterialcontainer,
    Loomresultpreviewcontainer,
    Blastfurnaceingredientcontainer,
    Smokeringredientcontainer,
    Trade2ingredient1container,
    Trade2ingredient2container,
    Trade2resultpreviewcontainer,
    Grindstoneinputcontainer,
    Grindstoneadditionalcontainer,
    Grindstoneresultpreviewcontainer,
    Stonecutterinputcontainer,
    Stonecutterresultpreviewcontainer,
    Cartographyinputcontainer,
    Cartographyadditionalcontainer,
    Cartographyresultpreviewcontainer,
    Barrelcontainer,
    Cursorcontainer,
    Createdoutputcontainer,
    Smithingtabletemplatecontainer,
    Crafterlevelentitycontainer,
    Dynamiccontainer,
    Recipefoodcontainer,
    Recipeblockscontainer,
    Recipefurnaceitemscontainer,
    Unknown(u8),
}

impl From<u8> for ContainerEnumName {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Anvilinputcontainer,
            1 => Self::Anvilmaterialcontainer,
            2 => Self::Anvilresultpreviewcontainer,
            3 => Self::Smithingtableinputcontainer,
            4 => Self::Smithingtablematerialcontainer,
            5 => Self::Smithingtableresultpreviewcontainer,
            6 => Self::Armorcontainer,
            7 => Self::Levelentitycontainer,
            8 => Self::Beaconpaymentcontainer,
            9 => Self::Brewingstandinputcontainer,
            10 => Self::Brewingstandresultcontainer,
            11 => Self::Brewingstandfuelcontainer,
            12 => Self::Combinedhotbarandinventorycontainer,
            13 => Self::Craftinginputcontainer,
            14 => Self::Craftingoutputpreviewcontainer,
            15 => Self::Recipeconstructioncontainer,
            16 => Self::Recipenaturecontainer,
            17 => Self::Recipeitemscontainer,
            18 => Self::Recipesearchcontainer,
            19 => Self::Recipesearchbarcontainer,
            20 => Self::Recipeequipmentcontainer,
            21 => Self::Recipebookcontainer,
            22 => Self::Enchantinginputcontainer,
            23 => Self::Enchantingmaterialcontainer,
            24 => Self::Furnacefuelcontainer,
            25 => Self::Furnaceingredientcontainer,
            26 => Self::Furnaceresultcontainer,
            27 => Self::Horseequipcontainer,
            28 => Self::Hotbarcontainer,
            29 => Self::Inventorycontainer,
            30 => Self::Shulkerboxcontainer,
            31 => Self::Tradeingredient1container,
            32 => Self::Tradeingredient2container,
            33 => Self::Traderesultpreviewcontainer,
            34 => Self::Offhandcontainer,
            35 => Self::Compoundcreatorinput,
            36 => Self::Compoundcreatoroutputpreview,
            37 => Self::Elementconstructoroutputpreview,
            38 => Self::Materialreducerinput,
            39 => Self::Materialreduceroutput,
            40 => Self::Labtableinput,
            41 => Self::Loominputcontainer,
            42 => Self::Loomdyecontainer,
            43 => Self::Loommaterialcontainer,
            44 => Self::Loomresultpreviewcontainer,
            45 => Self::Blastfurnaceingredientcontainer,
            46 => Self::Smokeringredientcontainer,
            47 => Self::Trade2ingredient1container,
            48 => Self::Trade2ingredient2container,
            49 => Self::Trade2resultpreviewcontainer,
            50 => Self::Grindstoneinputcontainer,
            51 => Self::Grindstoneadditionalcontainer,
            52 => Self::Grindstoneresultpreviewcontainer,
            53 => Self::Stonecutterinputcontainer,
            54 => Self::Stonecutterresultpreviewcontainer,
            55 => Self::Cartographyinputcontainer,
            56 => Self::Cartographyadditionalcontainer,
            57 => Self::Cartographyresultpreviewcontainer,
            58 => Self::Barrelcontainer,
            59 => Self::Cursorcontainer,
            60 => Self::Createdoutputcontainer,
            61 => Self::Smithingtabletemplatecontainer,
            62 => Self::Crafterlevelentitycontainer,
            63 => Self::Dynamiccontainer,
            64 => Self::Recipefoodcontainer,
            65 => Self::Recipeblockscontainer,
            66 => Self::Recipefurnaceitemscontainer,
            value => Self::Unknown(value),
        }
    }
}

impl ContainerEnumName {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Anvilinputcontainer => 0,
            Self::Anvilmaterialcontainer => 1,
            Self::Anvilresultpreviewcontainer => 2,
            Self::Smithingtableinputcontainer => 3,
            Self::Smithingtablematerialcontainer => 4,
            Self::Smithingtableresultpreviewcontainer => 5,
            Self::Armorcontainer => 6,
            Self::Levelentitycontainer => 7,
            Self::Beaconpaymentcontainer => 8,
            Self::Brewingstandinputcontainer => 9,
            Self::Brewingstandresultcontainer => 10,
            Self::Brewingstandfuelcontainer => 11,
            Self::Combinedhotbarandinventorycontainer => 12,
            Self::Craftinginputcontainer => 13,
            Self::Craftingoutputpreviewcontainer => 14,
            Self::Recipeconstructioncontainer => 15,
            Self::Recipenaturecontainer => 16,
            Self::Recipeitemscontainer => 17,
            Self::Recipesearchcontainer => 18,
            Self::Recipesearchbarcontainer => 19,
            Self::Recipeequipmentcontainer => 20,
            Self::Recipebookcontainer => 21,
            Self::Enchantinginputcontainer => 22,
            Self::Enchantingmaterialcontainer => 23,
            Self::Furnacefuelcontainer => 24,
            Self::Furnaceingredientcontainer => 25,
            Self::Furnaceresultcontainer => 26,
            Self::Horseequipcontainer => 27,
            Self::Hotbarcontainer => 28,
            Self::Inventorycontainer => 29,
            Self::Shulkerboxcontainer => 30,
            Self::Tradeingredient1container => 31,
            Self::Tradeingredient2container => 32,
            Self::Traderesultpreviewcontainer => 33,
            Self::Offhandcontainer => 34,
            Self::Compoundcreatorinput => 35,
            Self::Compoundcreatoroutputpreview => 36,
            Self::Elementconstructoroutputpreview => 37,
            Self::Materialreducerinput => 38,
            Self::Materialreduceroutput => 39,
            Self::Labtableinput => 40,
            Self::Loominputcontainer => 41,
            Self::Loomdyecontainer => 42,
            Self::Loommaterialcontainer => 43,
            Self::Loomresultpreviewcontainer => 44,
            Self::Blastfurnaceingredientcontainer => 45,
            Self::Smokeringredientcontainer => 46,
            Self::Trade2ingredient1container => 47,
            Self::Trade2ingredient2container => 48,
            Self::Trade2resultpreviewcontainer => 49,
            Self::Grindstoneinputcontainer => 50,
            Self::Grindstoneadditionalcontainer => 51,
            Self::Grindstoneresultpreviewcontainer => 52,
            Self::Stonecutterinputcontainer => 53,
            Self::Stonecutterresultpreviewcontainer => 54,
            Self::Cartographyinputcontainer => 55,
            Self::Cartographyadditionalcontainer => 56,
            Self::Cartographyresultpreviewcontainer => 57,
            Self::Barrelcontainer => 58,
            Self::Cursorcontainer => 59,
            Self::Createdoutputcontainer => 60,
            Self::Smithingtabletemplatecontainer => 61,
            Self::Crafterlevelentitycontainer => 62,
            Self::Dynamiccontainer => 63,
            Self::Recipefoodcontainer => 64,
            Self::Recipeblockscontainer => 65,
            Self::Recipefurnaceitemscontainer => 66,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ContainerEnumName> for u8 {
    fn from(value: ContainerEnumName) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ContainerEnumName {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ContainerEnumName {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

// Domain: creative

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum CreativeItemCategory {
    #[default]
    Construction,
    Nature,
    Equipment,
    Items,
    Itemcommandonly,
    Unknown(u8),
}

impl From<u8> for CreativeItemCategory {
    fn from(value: u8) -> Self {
        match value {
            1 => Self::Construction,
            2 => Self::Nature,
            3 => Self::Equipment,
            4 => Self::Items,
            5 => Self::Itemcommandonly,
            value => Self::Unknown(value),
        }
    }
}

impl CreativeItemCategory {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Construction => 1,
            Self::Nature => 2,
            Self::Equipment => 3,
            Self::Items => 4,
            Self::Itemcommandonly => 5,
            Self::Unknown(value) => value,
        }
    }
}

impl From<CreativeItemCategory> for u8 {
    fn from(value: CreativeItemCategory) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for CreativeItemCategory {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CreativeItemCategory {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

// Domain: education

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum EducationEditionOffer {
    #[default]
    None,
    Restofworld,
    ChinaDeprecated,
    Unknown(u32),
}

impl From<u32> for EducationEditionOffer {
    fn from(value: u32) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Restofworld,
            2 => Self::ChinaDeprecated,
            value => Self::Unknown(value),
        }
    }
}

impl EducationEditionOffer {
    pub fn to_raw(self) -> u32 {
        match self {
            Self::None => 0,
            Self::Restofworld => 1,
            Self::ChinaDeprecated => 2,
            Self::Unknown(value) => value,
        }
    }
}

impl From<EducationEditionOffer> for u32 {
    fn from(value: EducationEditionOffer) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for EducationEditionOffer {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::VarUInt(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for EducationEditionOffer {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::VarUInt as wire::Decode>::decode(reader)?.0,
        ))
    }
}

// Domain: enchant

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum EnchantType {
    #[default]
    Protection,
    Fireprotection,
    Featherfalling,
    Blastprotection,
    Projectileprotection,
    Thorns,
    Respiration,
    Depthstrider,
    Aquaaffinity,
    Sharpness,
    Smite,
    Baneofarthropods,
    Knockback,
    Fireaspect,
    Looting,
    Efficiency,
    Silktouch,
    Unbreaking,
    Fortune,
    Power,
    Punch,
    Flame,
    Infinity,
    Luckofthesea,
    Lure,
    Frostwalker,
    Mending,
    Curseofbinding,
    Curseofvanishing,
    Impaling,
    Riptide,
    Loyalty,
    Channeling,
    Multishot,
    Piercing,
    Quickcharge,
    Soulspeed,
    Swiftsneak,
    Windburst,
    Density,
    Breach,
    Lunge,
    Numenchantments,
    Invalidenchantment,
    Unknown(u8),
}

impl From<u8> for EnchantType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Protection,
            1 => Self::Fireprotection,
            2 => Self::Featherfalling,
            3 => Self::Blastprotection,
            4 => Self::Projectileprotection,
            5 => Self::Thorns,
            6 => Self::Respiration,
            7 => Self::Depthstrider,
            8 => Self::Aquaaffinity,
            9 => Self::Sharpness,
            10 => Self::Smite,
            11 => Self::Baneofarthropods,
            12 => Self::Knockback,
            13 => Self::Fireaspect,
            14 => Self::Looting,
            15 => Self::Efficiency,
            16 => Self::Silktouch,
            17 => Self::Unbreaking,
            18 => Self::Fortune,
            19 => Self::Power,
            20 => Self::Punch,
            21 => Self::Flame,
            22 => Self::Infinity,
            23 => Self::Luckofthesea,
            24 => Self::Lure,
            25 => Self::Frostwalker,
            26 => Self::Mending,
            27 => Self::Curseofbinding,
            28 => Self::Curseofvanishing,
            29 => Self::Impaling,
            30 => Self::Riptide,
            31 => Self::Loyalty,
            32 => Self::Channeling,
            33 => Self::Multishot,
            34 => Self::Piercing,
            35 => Self::Quickcharge,
            36 => Self::Soulspeed,
            37 => Self::Swiftsneak,
            38 => Self::Windburst,
            39 => Self::Density,
            40 => Self::Breach,
            41 => Self::Lunge,
            42 => Self::Numenchantments,
            43 => Self::Invalidenchantment,
            value => Self::Unknown(value),
        }
    }
}

impl EnchantType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Protection => 0,
            Self::Fireprotection => 1,
            Self::Featherfalling => 2,
            Self::Blastprotection => 3,
            Self::Projectileprotection => 4,
            Self::Thorns => 5,
            Self::Respiration => 6,
            Self::Depthstrider => 7,
            Self::Aquaaffinity => 8,
            Self::Sharpness => 9,
            Self::Smite => 10,
            Self::Baneofarthropods => 11,
            Self::Knockback => 12,
            Self::Fireaspect => 13,
            Self::Looting => 14,
            Self::Efficiency => 15,
            Self::Silktouch => 16,
            Self::Unbreaking => 17,
            Self::Fortune => 18,
            Self::Power => 19,
            Self::Punch => 20,
            Self::Flame => 21,
            Self::Infinity => 22,
            Self::Luckofthesea => 23,
            Self::Lure => 24,
            Self::Frostwalker => 25,
            Self::Mending => 26,
            Self::Curseofbinding => 27,
            Self::Curseofvanishing => 28,
            Self::Impaling => 29,
            Self::Riptide => 30,
            Self::Loyalty => 31,
            Self::Channeling => 32,
            Self::Multishot => 33,
            Self::Piercing => 34,
            Self::Quickcharge => 35,
            Self::Soulspeed => 36,
            Self::Swiftsneak => 37,
            Self::Windburst => 38,
            Self::Density => 39,
            Self::Breach => 40,
            Self::Lunge => 41,
            Self::Numenchantments => 42,
            Self::Invalidenchantment => 43,
            Self::Unknown(value) => value,
        }
    }
}

impl From<EnchantType> for u8 {
    fn from(value: EnchantType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for EnchantType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for EnchantType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

// Domain: furnace

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum FurnaceLayout {
    #[default]
    None,
    Inventoryonly,
    Default,
    Unknown(i32),
}

impl From<i32> for FurnaceLayout {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Inventoryonly,
            2 => Self::Default,
            value => Self::Unknown(value),
        }
    }
}

impl FurnaceLayout {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::None => 0,
            Self::Inventoryonly => 1,
            Self::Default => 2,
            Self::Unknown(value) => value,
        }
    }
}

impl From<FurnaceLayout> for i32 {
    fn from(value: FurnaceLayout) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for FurnaceLayout {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for FurnaceLayout {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum FurnaceLeftTabIndex {
    #[default]
    None,
    Recipefood,
    Recipeitems,
    Recipeblocks,
    Recipesearch,
    Inventory,
    Unknown(i32),
}

impl From<i32> for FurnaceLeftTabIndex {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Recipefood,
            2 => Self::Recipeitems,
            3 => Self::Recipeblocks,
            4 => Self::Recipesearch,
            5 => Self::Inventory,
            value => Self::Unknown(value),
        }
    }
}

impl FurnaceLeftTabIndex {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::None => 0,
            Self::Recipefood => 1,
            Self::Recipeitems => 2,
            Self::Recipeblocks => 3,
            Self::Recipesearch => 4,
            Self::Inventory => 5,
            Self::Unknown(value) => value,
        }
    }
}

impl From<FurnaceLeftTabIndex> for i32 {
    fn from(value: FurnaceLeftTabIndex) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for FurnaceLeftTabIndex {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for FurnaceLeftTabIndex {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum FurnaceType {
    #[default]
    None,
    Furnace,
    Blastfurnace,
    Smoker,
    Unknown(u8),
}

impl From<u8> for FurnaceType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Furnace,
            2 => Self::Blastfurnace,
            3 => Self::Smoker,
            value => Self::Unknown(value),
        }
    }
}

impl FurnaceType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Furnace => 1,
            Self::Blastfurnace => 2,
            Self::Smoker => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<FurnaceType> for u8 {
    fn from(value: FurnaceType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for FurnaceType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for FurnaceType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

// Domain: inventory

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum HandSlot {
    #[default]
    Mainhand,
    Offhand,
    Unknown(u8),
}

impl From<u8> for HandSlot {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Mainhand,
            1 => Self::Offhand,
            value => Self::Unknown(value),
        }
    }
}

impl HandSlot {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Mainhand => 0,
            Self::Offhand => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<HandSlot> for u8 {
    fn from(value: HandSlot) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for HandSlot {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for HandSlot {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum InventoryLayout {
    #[default]
    None,
    Inventoryonly,
    Default,
    Recipebookonly,
    Unknown(i32),
}

impl From<i32> for InventoryLayout {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Inventoryonly,
            2 => Self::Default,
            3 => Self::Recipebookonly,
            value => Self::Unknown(value),
        }
    }
}

impl InventoryLayout {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::None => 0,
            Self::Inventoryonly => 1,
            Self::Default => 2,
            Self::Recipebookonly => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<InventoryLayout> for i32 {
    fn from(value: InventoryLayout) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for InventoryLayout {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for InventoryLayout {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum InventoryLeftTabIndex {
    #[default]
    None,
    Recipeconstruction,
    Recipeequipment,
    Recipeitems,
    Recipenature,
    Recipesearch,
    Survival,
    Unknown(i32),
}

impl From<i32> for InventoryLeftTabIndex {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Recipeconstruction,
            2 => Self::Recipeequipment,
            3 => Self::Recipeitems,
            4 => Self::Recipenature,
            5 => Self::Recipesearch,
            6 => Self::Survival,
            value => Self::Unknown(value),
        }
    }
}

impl InventoryLeftTabIndex {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::None => 0,
            Self::Recipeconstruction => 1,
            Self::Recipeequipment => 2,
            Self::Recipeitems => 3,
            Self::Recipenature => 4,
            Self::Recipesearch => 5,
            Self::Survival => 6,
            Self::Unknown(value) => value,
        }
    }
}

impl From<InventoryLeftTabIndex> for i32 {
    fn from(value: InventoryLeftTabIndex) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for InventoryLeftTabIndex {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for InventoryLeftTabIndex {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum InventoryRightTabIndex {
    #[default]
    None,
    Fullscreen,
    Crafting,
    Armor,
    Unknown(i32),
}

impl From<i32> for InventoryRightTabIndex {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Fullscreen,
            2 => Self::Crafting,
            3 => Self::Armor,
            value => Self::Unknown(value),
        }
    }
}

impl InventoryRightTabIndex {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::None => 0,
            Self::Fullscreen => 1,
            Self::Crafting => 2,
            Self::Armor => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<InventoryRightTabIndex> for i32 {
    fn from(value: InventoryRightTabIndex) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for InventoryRightTabIndex {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for InventoryRightTabIndex {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum InventorySourceInventorySourceFlags {
    #[default]
    NoFlag,
    WorldInteractionRandom,
    Unknown(u32),
}

impl From<u32> for InventorySourceInventorySourceFlags {
    fn from(value: u32) -> Self {
        match value {
            0 => Self::NoFlag,
            1 => Self::WorldInteractionRandom,
            value => Self::Unknown(value),
        }
    }
}

impl InventorySourceInventorySourceFlags {
    pub fn to_raw(self) -> u32 {
        match self {
            Self::NoFlag => 0,
            Self::WorldInteractionRandom => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<InventorySourceInventorySourceFlags> for u32 {
    fn from(value: InventorySourceInventorySourceFlags) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for InventorySourceInventorySourceFlags {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::VarUInt(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for InventorySourceInventorySourceFlags {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::VarUInt as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum InventorySourceType {
    #[default]
    ContainerInventory,
    GlobalInventory,
    WorldInteraction,
    CreativeInventory,
    NonImplementedFeatureTodo,
    Unknown(u32),
}

impl From<u32> for InventorySourceType {
    fn from(value: u32) -> Self {
        match value {
            0 => Self::ContainerInventory,
            1 => Self::GlobalInventory,
            2 => Self::WorldInteraction,
            3 => Self::CreativeInventory,
            99999 => Self::NonImplementedFeatureTodo,
            value => Self::Unknown(value),
        }
    }
}

impl InventorySourceType {
    pub fn to_raw(self) -> u32 {
        match self {
            Self::ContainerInventory => 0,
            Self::GlobalInventory => 1,
            Self::WorldInteraction => 2,
            Self::CreativeInventory => 3,
            Self::NonImplementedFeatureTodo => 99999,
            Self::Unknown(value) => value,
        }
    }
}

impl From<InventorySourceType> for u32 {
    fn from(value: InventorySourceType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for InventorySourceType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::VarUInt(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for InventorySourceType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::VarUInt as wire::Decode>::decode(reader)?.0,
        ))
    }
}

// Domain: item

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ItemReleaseInventoryTransactionActionType {
    #[default]
    Release,
    Use,
    Unknown(i32),
}

impl From<i32> for ItemReleaseInventoryTransactionActionType {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Release,
            1 => Self::Use,
            value => Self::Unknown(value),
        }
    }
}

impl ItemReleaseInventoryTransactionActionType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Release => 0,
            Self::Use => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ItemReleaseInventoryTransactionActionType> for i32 {
    fn from(value: ItemReleaseInventoryTransactionActionType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ItemReleaseInventoryTransactionActionType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ItemReleaseInventoryTransactionActionType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ItemUseInventoryTransactionActionType {
    #[default]
    Place,
    Use,
    Destroy,
    UseAsAttack,
    Unknown(i32),
}

impl From<i32> for ItemUseInventoryTransactionActionType {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Place,
            1 => Self::Use,
            2 => Self::Destroy,
            3 => Self::UseAsAttack,
            value => Self::Unknown(value),
        }
    }
}

impl ItemUseInventoryTransactionActionType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Place => 0,
            Self::Use => 1,
            Self::Destroy => 2,
            Self::UseAsAttack => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ItemUseInventoryTransactionActionType> for i32 {
    fn from(value: ItemUseInventoryTransactionActionType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ItemUseInventoryTransactionActionType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ItemUseInventoryTransactionActionType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ItemUseInventoryTransactionClientCooldownState {
    #[default]
    Off,
    On,
    Unknown(u8),
}

impl From<u8> for ItemUseInventoryTransactionClientCooldownState {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Off,
            1 => Self::On,
            value => Self::Unknown(value),
        }
    }
}

impl ItemUseInventoryTransactionClientCooldownState {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Off => 0,
            Self::On => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ItemUseInventoryTransactionClientCooldownState> for u8 {
    fn from(value: ItemUseInventoryTransactionClientCooldownState) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ItemUseInventoryTransactionClientCooldownState {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ItemUseInventoryTransactionClientCooldownState {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ItemUseInventoryTransactionPredictedResult {
    #[default]
    Failure,
    Success,
    Unknown(u8),
}

impl From<u8> for ItemUseInventoryTransactionPredictedResult {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Failure,
            1 => Self::Success,
            value => Self::Unknown(value),
        }
    }
}

impl ItemUseInventoryTransactionPredictedResult {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Failure => 0,
            Self::Success => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ItemUseInventoryTransactionPredictedResult> for u8 {
    fn from(value: ItemUseInventoryTransactionPredictedResult) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ItemUseInventoryTransactionPredictedResult {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ItemUseInventoryTransactionPredictedResult {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ItemUseInventoryTransactionTriggerType {
    #[default]
    Unknown,
    PlayerInput,
    SimulationTick,
    Unknown2(u8),
}

impl From<u8> for ItemUseInventoryTransactionTriggerType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Unknown,
            1 => Self::PlayerInput,
            2 => Self::SimulationTick,
            value => Self::Unknown2(value),
        }
    }
}

impl ItemUseInventoryTransactionTriggerType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Unknown => 0,
            Self::PlayerInput => 1,
            Self::SimulationTick => 2,
            Self::Unknown2(value) => value,
        }
    }
}

impl From<ItemUseInventoryTransactionTriggerType> for u8 {
    fn from(value: ItemUseInventoryTransactionTriggerType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ItemUseInventoryTransactionTriggerType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ItemUseInventoryTransactionTriggerType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ItemUseOnActorInventoryTransactionActionType {
    #[default]
    Interact,
    Attack,
    ItemInteract,
    Unknown(i32),
}

impl From<i32> for ItemUseOnActorInventoryTransactionActionType {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Interact,
            1 => Self::Attack,
            2 => Self::ItemInteract,
            value => Self::Unknown(value),
        }
    }
}

impl ItemUseOnActorInventoryTransactionActionType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Interact => 0,
            Self::Attack => 1,
            Self::ItemInteract => 2,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ItemUseOnActorInventoryTransactionActionType> for i32 {
    fn from(value: ItemUseOnActorInventoryTransactionActionType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ItemUseOnActorInventoryTransactionActionType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ItemUseOnActorInventoryTransactionActionType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ItemVersion {
    #[default]
    Legacy,
    Datadriven,
    None,
    Unknown(i32),
}

impl From<i32> for ItemVersion {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Legacy,
            1 => Self::Datadriven,
            2 => Self::None,
            value => Self::Unknown(value),
        }
    }
}

impl ItemVersion {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Legacy => 0,
            Self::Datadriven => 1,
            Self::None => 2,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ItemVersion> for i32 {
    fn from(value: ItemVersion) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ItemVersion {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ItemVersion {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

// Domain: item_descriptor

/// ItemDescriptor represents a type of item descriptor. This is one of the concrete types below. It is an
/// alias of Marshaler.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ItemDescriptorType {
    #[default]
    Empty,
    Itemname,
    MoLang,
    Itemtag,
    Unknown(u8),
}

impl From<u8> for ItemDescriptorType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Empty,
            1 => Self::Itemname,
            2 => Self::MoLang,
            3 => Self::Itemtag,
            value => Self::Unknown(value),
        }
    }
}

impl ItemDescriptorType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Empty => 0,
            Self::Itemname => 1,
            Self::MoLang => 2,
            Self::Itemtag => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ItemDescriptorType> for u8 {
    fn from(value: ItemDescriptorType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ItemDescriptorType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ItemDescriptorType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

// Domain: item_stack

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ItemStackNetResult {
    #[default]
    Success,
    Error,
    Invalidrequestactiontype,
    Actionrequestnotallowed,
    Screenhandlerendrequestfailed,
    Itemrequestactionhandlercommitfailed,
    Invalidrequestcraftactiontype,
    Invalidcraftrequest,
    Invalidcraftrequestscreen,
    Invalidcraftresult,
    Invalidcraftresultindex,
    Invalidcraftresultitem,
    Invaliditemnetid,
    Missingcreatedoutputcontainer,
    Failedtosetcreateditemoutputslot,
    Requestalreadyinprogress,
    Failedtoinitsparsecontainer,
    Resulttransferfailed,
    Expecteditemslotnotfullyconsumed,
    Expectedanywhereitemnotfullyconsumed,
    Itemalreadyconsumedfromslot,
    Consumedtoomuchfromslot,
    Mismatchslotexpectedconsumeditem,
    Mismatchslotexpectedconsumeditemnetidvariant,
    Failedtomatchexpectedslotconsumeditem,
    Failedtomatchexpectedallowedanywhereconsumeditem,
    Consumeditemoutofallowedslotrange,
    Consumeditemnotallowed,
    Playernotincreativemode,
    Invalidexperimentalreciperequest,
    Failedtocraftcreative,
    Failedtogetlevelrecipe,
    Failedtofindrecipebynetid,
    Mismatchedcraftingsize,
    Missinginputsparsecontainer,
    Mismatchedrecipeforinputgriditems,
    Emptycraftresults,
    Failedtoenchant,
    Missinginputitem,
    Insufficientplayerleveltoenchant,
    Missingmaterialitem,
    Missingactor,
    Unknownprimaryeffect,
    Primaryeffectoutofrange,
    Primaryeffectunavailable,
    Secondaryeffectoutofrange,
    Secondaryeffectunavailable,
    Dstcontainerequaltocreatedoutputcontainer,
    Dstcontainerandslotequaltosrccontainerandslot,
    Failedtovalidatesrcslot,
    Failedtovalidatedstslot,
    Invalidadjustedamount,
    Invaliditemsettype,
    Invalidtransferamount,
    Cannotswapitem,
    Cannotplaceitem,
    Unhandleditemsettype,
    Invalidremovedamount,
    Invalidregion,
    Cannotdropitem,
    Cannotdestroyitem,
    Invalidsourcecontainer,
    Itemnotconsumed,
    Invalidnumcrafts,
    Invalidcraftresultstacksize,
    Cannotremoveitem,
    Cannotconsumeitem,
    Screenstackerror,
    Unknown(u8),
}

impl From<u8> for ItemStackNetResult {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Success,
            1 => Self::Error,
            2 => Self::Invalidrequestactiontype,
            3 => Self::Actionrequestnotallowed,
            4 => Self::Screenhandlerendrequestfailed,
            5 => Self::Itemrequestactionhandlercommitfailed,
            6 => Self::Invalidrequestcraftactiontype,
            7 => Self::Invalidcraftrequest,
            8 => Self::Invalidcraftrequestscreen,
            9 => Self::Invalidcraftresult,
            10 => Self::Invalidcraftresultindex,
            11 => Self::Invalidcraftresultitem,
            12 => Self::Invaliditemnetid,
            13 => Self::Missingcreatedoutputcontainer,
            14 => Self::Failedtosetcreateditemoutputslot,
            15 => Self::Requestalreadyinprogress,
            16 => Self::Failedtoinitsparsecontainer,
            17 => Self::Resulttransferfailed,
            18 => Self::Expecteditemslotnotfullyconsumed,
            19 => Self::Expectedanywhereitemnotfullyconsumed,
            20 => Self::Itemalreadyconsumedfromslot,
            21 => Self::Consumedtoomuchfromslot,
            22 => Self::Mismatchslotexpectedconsumeditem,
            23 => Self::Mismatchslotexpectedconsumeditemnetidvariant,
            24 => Self::Failedtomatchexpectedslotconsumeditem,
            25 => Self::Failedtomatchexpectedallowedanywhereconsumeditem,
            26 => Self::Consumeditemoutofallowedslotrange,
            27 => Self::Consumeditemnotallowed,
            28 => Self::Playernotincreativemode,
            29 => Self::Invalidexperimentalreciperequest,
            30 => Self::Failedtocraftcreative,
            31 => Self::Failedtogetlevelrecipe,
            32 => Self::Failedtofindrecipebynetid,
            33 => Self::Mismatchedcraftingsize,
            34 => Self::Missinginputsparsecontainer,
            35 => Self::Mismatchedrecipeforinputgriditems,
            36 => Self::Emptycraftresults,
            37 => Self::Failedtoenchant,
            38 => Self::Missinginputitem,
            39 => Self::Insufficientplayerleveltoenchant,
            40 => Self::Missingmaterialitem,
            41 => Self::Missingactor,
            42 => Self::Unknownprimaryeffect,
            43 => Self::Primaryeffectoutofrange,
            44 => Self::Primaryeffectunavailable,
            45 => Self::Secondaryeffectoutofrange,
            46 => Self::Secondaryeffectunavailable,
            47 => Self::Dstcontainerequaltocreatedoutputcontainer,
            48 => Self::Dstcontainerandslotequaltosrccontainerandslot,
            49 => Self::Failedtovalidatesrcslot,
            50 => Self::Failedtovalidatedstslot,
            51 => Self::Invalidadjustedamount,
            52 => Self::Invaliditemsettype,
            53 => Self::Invalidtransferamount,
            54 => Self::Cannotswapitem,
            55 => Self::Cannotplaceitem,
            56 => Self::Unhandleditemsettype,
            57 => Self::Invalidremovedamount,
            58 => Self::Invalidregion,
            59 => Self::Cannotdropitem,
            60 => Self::Cannotdestroyitem,
            61 => Self::Invalidsourcecontainer,
            62 => Self::Itemnotconsumed,
            63 => Self::Invalidnumcrafts,
            64 => Self::Invalidcraftresultstacksize,
            65 => Self::Cannotremoveitem,
            66 => Self::Cannotconsumeitem,
            67 => Self::Screenstackerror,
            value => Self::Unknown(value),
        }
    }
}

impl ItemStackNetResult {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Success => 0,
            Self::Error => 1,
            Self::Invalidrequestactiontype => 2,
            Self::Actionrequestnotallowed => 3,
            Self::Screenhandlerendrequestfailed => 4,
            Self::Itemrequestactionhandlercommitfailed => 5,
            Self::Invalidrequestcraftactiontype => 6,
            Self::Invalidcraftrequest => 7,
            Self::Invalidcraftrequestscreen => 8,
            Self::Invalidcraftresult => 9,
            Self::Invalidcraftresultindex => 10,
            Self::Invalidcraftresultitem => 11,
            Self::Invaliditemnetid => 12,
            Self::Missingcreatedoutputcontainer => 13,
            Self::Failedtosetcreateditemoutputslot => 14,
            Self::Requestalreadyinprogress => 15,
            Self::Failedtoinitsparsecontainer => 16,
            Self::Resulttransferfailed => 17,
            Self::Expecteditemslotnotfullyconsumed => 18,
            Self::Expectedanywhereitemnotfullyconsumed => 19,
            Self::Itemalreadyconsumedfromslot => 20,
            Self::Consumedtoomuchfromslot => 21,
            Self::Mismatchslotexpectedconsumeditem => 22,
            Self::Mismatchslotexpectedconsumeditemnetidvariant => 23,
            Self::Failedtomatchexpectedslotconsumeditem => 24,
            Self::Failedtomatchexpectedallowedanywhereconsumeditem => 25,
            Self::Consumeditemoutofallowedslotrange => 26,
            Self::Consumeditemnotallowed => 27,
            Self::Playernotincreativemode => 28,
            Self::Invalidexperimentalreciperequest => 29,
            Self::Failedtocraftcreative => 30,
            Self::Failedtogetlevelrecipe => 31,
            Self::Failedtofindrecipebynetid => 32,
            Self::Mismatchedcraftingsize => 33,
            Self::Missinginputsparsecontainer => 34,
            Self::Mismatchedrecipeforinputgriditems => 35,
            Self::Emptycraftresults => 36,
            Self::Failedtoenchant => 37,
            Self::Missinginputitem => 38,
            Self::Insufficientplayerleveltoenchant => 39,
            Self::Missingmaterialitem => 40,
            Self::Missingactor => 41,
            Self::Unknownprimaryeffect => 42,
            Self::Primaryeffectoutofrange => 43,
            Self::Primaryeffectunavailable => 44,
            Self::Secondaryeffectoutofrange => 45,
            Self::Secondaryeffectunavailable => 46,
            Self::Dstcontainerequaltocreatedoutputcontainer => 47,
            Self::Dstcontainerandslotequaltosrccontainerandslot => 48,
            Self::Failedtovalidatesrcslot => 49,
            Self::Failedtovalidatedstslot => 50,
            Self::Invalidadjustedamount => 51,
            Self::Invaliditemsettype => 52,
            Self::Invalidtransferamount => 53,
            Self::Cannotswapitem => 54,
            Self::Cannotplaceitem => 55,
            Self::Unhandleditemsettype => 56,
            Self::Invalidremovedamount => 57,
            Self::Invalidregion => 58,
            Self::Cannotdropitem => 59,
            Self::Cannotdestroyitem => 60,
            Self::Invalidsourcecontainer => 61,
            Self::Itemnotconsumed => 62,
            Self::Invalidnumcrafts => 63,
            Self::Invalidcraftresultstacksize => 64,
            Self::Cannotremoveitem => 65,
            Self::Cannotconsumeitem => 66,
            Self::Screenstackerror => 67,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ItemStackNetResult> for u8 {
    fn from(value: ItemStackNetResult) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ItemStackNetResult {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ItemStackNetResult {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ItemStackRequestActionType {
    #[default]
    Take,
    Place,
    Swap,
    Drop,
    Destroy,
    Consume,
    Create,
    Placeinitemcontainer,
    Takefromitemcontainer,
    Screenlabtablecombine,
    Screenbeaconpayment,
    Screenhudmineblock,
    Craftrecipe,
    Craftrecipeauto,
    Craftcreative,
    Craftrecipeoptional,
    Craftrepairanddisenchant,
    Craftloom,
    Craftnonimplemented,
    Craftresults,
    Unknown(u8),
}

impl From<u8> for ItemStackRequestActionType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Take,
            1 => Self::Place,
            2 => Self::Swap,
            3 => Self::Drop,
            4 => Self::Destroy,
            5 => Self::Consume,
            6 => Self::Create,
            7 => Self::Placeinitemcontainer,
            8 => Self::Takefromitemcontainer,
            9 => Self::Screenlabtablecombine,
            10 => Self::Screenbeaconpayment,
            11 => Self::Screenhudmineblock,
            12 => Self::Craftrecipe,
            13 => Self::Craftrecipeauto,
            14 => Self::Craftcreative,
            15 => Self::Craftrecipeoptional,
            16 => Self::Craftrepairanddisenchant,
            17 => Self::Craftloom,
            18 => Self::Craftnonimplemented,
            19 => Self::Craftresults,
            value => Self::Unknown(value),
        }
    }
}

impl ItemStackRequestActionType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Take => 0,
            Self::Place => 1,
            Self::Swap => 2,
            Self::Drop => 3,
            Self::Destroy => 4,
            Self::Consume => 5,
            Self::Create => 6,
            Self::Placeinitemcontainer => 7,
            Self::Takefromitemcontainer => 8,
            Self::Screenlabtablecombine => 9,
            Self::Screenbeaconpayment => 10,
            Self::Screenhudmineblock => 11,
            Self::Craftrecipe => 12,
            Self::Craftrecipeauto => 13,
            Self::Craftcreative => 14,
            Self::Craftrecipeoptional => 15,
            Self::Craftrepairanddisenchant => 16,
            Self::Craftloom => 17,
            Self::Craftnonimplemented => 18,
            Self::Craftresults => 19,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ItemStackRequestActionType> for u8 {
    fn from(value: ItemStackRequestActionType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ItemStackRequestActionType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ItemStackRequestActionType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

// Domain: map

/// MapDecoration is a fixed decoration on a map: Its position or other properties do not change automatically
/// client-side.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum MapDecorationType {
    #[default]
    Markerwhite,
    Markergreen,
    Markerred,
    Markerblue,
    Xwhite,
    Trianglered,
    Squarewhite,
    Markersign,
    Markerpink,
    Markerorange,
    Markeryellow,
    Markerteal,
    Trianglegreen,
    Smallsquarewhite,
    Mansion,
    Monument,
    Nodraw,
    Villagedesert,
    Villageplains,
    Villagesavanna,
    Villagesnowy,
    Villagetaiga,
    Jungletemple,
    Witchhut,
    Trialchambers,
    Abandonedcamp,
    Buriedancientcity,
    Buriedmineshaft,
    Desertpyramid,
    Warmoceanruins,
    Count,
    Unknown(i8),
}

impl From<i8> for MapDecorationType {
    fn from(value: i8) -> Self {
        match value {
            0 => Self::Markerwhite,
            1 => Self::Markergreen,
            2 => Self::Markerred,
            3 => Self::Markerblue,
            4 => Self::Xwhite,
            5 => Self::Trianglered,
            6 => Self::Squarewhite,
            7 => Self::Markersign,
            8 => Self::Markerpink,
            9 => Self::Markerorange,
            10 => Self::Markeryellow,
            11 => Self::Markerteal,
            12 => Self::Trianglegreen,
            13 => Self::Smallsquarewhite,
            14 => Self::Mansion,
            15 => Self::Monument,
            16 => Self::Nodraw,
            17 => Self::Villagedesert,
            18 => Self::Villageplains,
            19 => Self::Villagesavanna,
            20 => Self::Villagesnowy,
            21 => Self::Villagetaiga,
            22 => Self::Jungletemple,
            23 => Self::Witchhut,
            24 => Self::Trialchambers,
            25 => Self::Abandonedcamp,
            26 => Self::Buriedancientcity,
            27 => Self::Buriedmineshaft,
            28 => Self::Desertpyramid,
            29 => Self::Warmoceanruins,
            30 => Self::Count,
            value => Self::Unknown(value),
        }
    }
}

impl MapDecorationType {
    pub fn to_raw(self) -> i8 {
        match self {
            Self::Markerwhite => 0,
            Self::Markergreen => 1,
            Self::Markerred => 2,
            Self::Markerblue => 3,
            Self::Xwhite => 4,
            Self::Trianglered => 5,
            Self::Squarewhite => 6,
            Self::Markersign => 7,
            Self::Markerpink => 8,
            Self::Markerorange => 9,
            Self::Markeryellow => 10,
            Self::Markerteal => 11,
            Self::Trianglegreen => 12,
            Self::Smallsquarewhite => 13,
            Self::Mansion => 14,
            Self::Monument => 15,
            Self::Nodraw => 16,
            Self::Villagedesert => 17,
            Self::Villageplains => 18,
            Self::Villagesavanna => 19,
            Self::Villagesnowy => 20,
            Self::Villagetaiga => 21,
            Self::Jungletemple => 22,
            Self::Witchhut => 23,
            Self::Trialchambers => 24,
            Self::Abandonedcamp => 25,
            Self::Buriedancientcity => 26,
            Self::Buriedmineshaft => 27,
            Self::Desertpyramid => 28,
            Self::Warmoceanruins => 29,
            Self::Count => 30,
            Self::Unknown(value) => value,
        }
    }
}

impl From<MapDecorationType> for i8 {
    fn from(value: MapDecorationType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for MapDecorationType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::I8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for MapDecorationType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::I8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum MapItemTrackedActorType {
    #[default]
    Entity,
    Blockentity,
    Other,
    Unknown(i32),
}

impl From<i32> for MapItemTrackedActorType {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Entity,
            1 => Self::Blockentity,
            2 => Self::Other,
            value => Self::Unknown(value),
        }
    }
}

impl MapItemTrackedActorType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Entity => 0,
            Self::Blockentity => 1,
            Self::Other => 2,
            Self::Unknown(value) => value,
        }
    }
}

impl From<MapItemTrackedActorType> for i32 {
    fn from(value: MapItemTrackedActorType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for MapItemTrackedActorType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::I32LE(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for MapItemTrackedActorType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::I32LE as wire::Decode>::decode(reader)?.0))
    }
}

// Domain: memory

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum MemoryCategory {
    #[default]
    Unknown,
    InvalidSizeunknown,
    Actor,
    Actoranimation,
    Actorrendering,
    Blocktickingqueues,
    BiomeStorage,
    Blobs,
    Cereal,
    Circuitsystem,
    Client,
    Commands,
    Dbstorage,
    Debug,
    Documentation,
    Ecssystems,
    Fmod,
    Fonts,
    Imgui,
    Input,
    Jsonui,
    JsonuiControlfactoryJson,
    JsonuiControltree,
    JsonuiControltreeControlelement,
    JsonuiControltreePopulatedatabinding,
    JsonuiControltreePopulatefocus,
    JsonuiControltreePopulatelayout,
    JsonuiControltreePopulateother,
    JsonuiControltreePopulatesprite,
    JsonuiControltreePopulatetext,
    JsonuiControltreePopulatetts,
    JsonuiControltreeVisibility,
    JsonuiCreateui,
    JsonuiDefs,
    JsonuiLayoutmanager,
    JsonuiLayoutmanagerRemovedependencies,
    JsonuiLayoutmanagerInitvariable,
    Languages,
    Level,
    Levelstructures,
    Levelchunk,
    Levelchunkgen,
    Levelchunkgenthreadlocal,
    Lightvolumemanager,
    Network,
    Marketplace,
    MaterialDragoncompileddefinition,
    MaterialDragonmaterial,
    MaterialDragonresource,
    MaterialDragonuniformmap,
    MaterialRendermaterial,
    MaterialRendermaterialgroup,
    MaterialVariationmanager,
    MoLang,
    Oreui,
    OreuiClient,
    PersonaPieces,
    PersonaAnimations,
    PersonaCharacters,
    PersonaSkinpacks,
    PersonaRepo,
    Player,
    Renderchunk,
    RenderchunkIndexbuffer,
    RenderchunkVertexbuffer,
    Rendering,
    RenderingBgfxinit,
    RenderingBgfxstartframe,
    RenderingBlocktessellator,
    RenderingEndframe,
    RenderingGraphicstasksinit,
    RenderingLibrary,
    RenderingPolygonoperatorpool,
    RenderingPbrtexturedata,
    RenderingRenderregistry,
    RenderingSetup,
    RenderingVertices,
    Requestlog,
    Resourcepacks,
    Sound,
    SubchunkBiomedata,
    SubchunkBlockdata,
    SubchunkLightdata,
    Textures,
    Weatherrenderer,
    WorldGenerator,
    Tasks,
    Test,
    TestLoadtesttags,
    Scripting,
    ScriptingRuntime,
    ScriptingContext,
    ScriptingContextBindingsMc,
    ScriptingContextBindingsGt,
    ScriptingContextRun,
    Datadrivenui,
    DatadrivenuiDefs,
    Gameface,
    GamefaceSystem,
    GamefaceDom,
    GamefaceCss,
    GamefaceDisplay,
    GamefaceTempallocator,
    GamefacePoolallocator,
    GamefaceDump,
    GamefaceMedia,
    GamefaceJson,
    GamefaceScriptengine,
    GamefaceScript,
    GamefaceLayout,
    Unknown2(u8),
}

impl From<u8> for MemoryCategory {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Unknown,
            1 => Self::InvalidSizeunknown,
            2 => Self::Actor,
            3 => Self::Actoranimation,
            4 => Self::Actorrendering,
            5 => Self::Blocktickingqueues,
            6 => Self::BiomeStorage,
            7 => Self::Blobs,
            8 => Self::Cereal,
            9 => Self::Circuitsystem,
            10 => Self::Client,
            11 => Self::Commands,
            12 => Self::Dbstorage,
            13 => Self::Debug,
            14 => Self::Documentation,
            15 => Self::Ecssystems,
            16 => Self::Fmod,
            17 => Self::Fonts,
            18 => Self::Imgui,
            19 => Self::Input,
            20 => Self::Jsonui,
            21 => Self::JsonuiControlfactoryJson,
            22 => Self::JsonuiControltree,
            23 => Self::JsonuiControltreeControlelement,
            24 => Self::JsonuiControltreePopulatedatabinding,
            25 => Self::JsonuiControltreePopulatefocus,
            26 => Self::JsonuiControltreePopulatelayout,
            27 => Self::JsonuiControltreePopulateother,
            28 => Self::JsonuiControltreePopulatesprite,
            29 => Self::JsonuiControltreePopulatetext,
            30 => Self::JsonuiControltreePopulatetts,
            31 => Self::JsonuiControltreeVisibility,
            32 => Self::JsonuiCreateui,
            33 => Self::JsonuiDefs,
            34 => Self::JsonuiLayoutmanager,
            35 => Self::JsonuiLayoutmanagerRemovedependencies,
            36 => Self::JsonuiLayoutmanagerInitvariable,
            37 => Self::Languages,
            38 => Self::Level,
            39 => Self::Levelstructures,
            40 => Self::Levelchunk,
            41 => Self::Levelchunkgen,
            42 => Self::Levelchunkgenthreadlocal,
            43 => Self::Lightvolumemanager,
            44 => Self::Network,
            45 => Self::Marketplace,
            46 => Self::MaterialDragoncompileddefinition,
            47 => Self::MaterialDragonmaterial,
            48 => Self::MaterialDragonresource,
            49 => Self::MaterialDragonuniformmap,
            50 => Self::MaterialRendermaterial,
            51 => Self::MaterialRendermaterialgroup,
            52 => Self::MaterialVariationmanager,
            53 => Self::MoLang,
            54 => Self::Oreui,
            55 => Self::OreuiClient,
            56 => Self::PersonaPieces,
            57 => Self::PersonaAnimations,
            58 => Self::PersonaCharacters,
            59 => Self::PersonaSkinpacks,
            60 => Self::PersonaRepo,
            61 => Self::Player,
            62 => Self::Renderchunk,
            63 => Self::RenderchunkIndexbuffer,
            64 => Self::RenderchunkVertexbuffer,
            65 => Self::Rendering,
            66 => Self::RenderingBgfxinit,
            67 => Self::RenderingBgfxstartframe,
            68 => Self::RenderingBlocktessellator,
            69 => Self::RenderingEndframe,
            70 => Self::RenderingGraphicstasksinit,
            71 => Self::RenderingLibrary,
            72 => Self::RenderingPolygonoperatorpool,
            73 => Self::RenderingPbrtexturedata,
            74 => Self::RenderingRenderregistry,
            75 => Self::RenderingSetup,
            76 => Self::RenderingVertices,
            77 => Self::Requestlog,
            78 => Self::Resourcepacks,
            79 => Self::Sound,
            80 => Self::SubchunkBiomedata,
            81 => Self::SubchunkBlockdata,
            82 => Self::SubchunkLightdata,
            83 => Self::Textures,
            84 => Self::Weatherrenderer,
            85 => Self::WorldGenerator,
            86 => Self::Tasks,
            87 => Self::Test,
            88 => Self::TestLoadtesttags,
            89 => Self::Scripting,
            90 => Self::ScriptingRuntime,
            91 => Self::ScriptingContext,
            92 => Self::ScriptingContextBindingsMc,
            93 => Self::ScriptingContextBindingsGt,
            94 => Self::ScriptingContextRun,
            95 => Self::Datadrivenui,
            96 => Self::DatadrivenuiDefs,
            97 => Self::Gameface,
            98 => Self::GamefaceSystem,
            99 => Self::GamefaceDom,
            100 => Self::GamefaceCss,
            101 => Self::GamefaceDisplay,
            102 => Self::GamefaceTempallocator,
            103 => Self::GamefacePoolallocator,
            104 => Self::GamefaceDump,
            105 => Self::GamefaceMedia,
            106 => Self::GamefaceJson,
            107 => Self::GamefaceScriptengine,
            108 => Self::GamefaceScript,
            109 => Self::GamefaceLayout,
            value => Self::Unknown2(value),
        }
    }
}

impl MemoryCategory {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Unknown => 0,
            Self::InvalidSizeunknown => 1,
            Self::Actor => 2,
            Self::Actoranimation => 3,
            Self::Actorrendering => 4,
            Self::Blocktickingqueues => 5,
            Self::BiomeStorage => 6,
            Self::Blobs => 7,
            Self::Cereal => 8,
            Self::Circuitsystem => 9,
            Self::Client => 10,
            Self::Commands => 11,
            Self::Dbstorage => 12,
            Self::Debug => 13,
            Self::Documentation => 14,
            Self::Ecssystems => 15,
            Self::Fmod => 16,
            Self::Fonts => 17,
            Self::Imgui => 18,
            Self::Input => 19,
            Self::Jsonui => 20,
            Self::JsonuiControlfactoryJson => 21,
            Self::JsonuiControltree => 22,
            Self::JsonuiControltreeControlelement => 23,
            Self::JsonuiControltreePopulatedatabinding => 24,
            Self::JsonuiControltreePopulatefocus => 25,
            Self::JsonuiControltreePopulatelayout => 26,
            Self::JsonuiControltreePopulateother => 27,
            Self::JsonuiControltreePopulatesprite => 28,
            Self::JsonuiControltreePopulatetext => 29,
            Self::JsonuiControltreePopulatetts => 30,
            Self::JsonuiControltreeVisibility => 31,
            Self::JsonuiCreateui => 32,
            Self::JsonuiDefs => 33,
            Self::JsonuiLayoutmanager => 34,
            Self::JsonuiLayoutmanagerRemovedependencies => 35,
            Self::JsonuiLayoutmanagerInitvariable => 36,
            Self::Languages => 37,
            Self::Level => 38,
            Self::Levelstructures => 39,
            Self::Levelchunk => 40,
            Self::Levelchunkgen => 41,
            Self::Levelchunkgenthreadlocal => 42,
            Self::Lightvolumemanager => 43,
            Self::Network => 44,
            Self::Marketplace => 45,
            Self::MaterialDragoncompileddefinition => 46,
            Self::MaterialDragonmaterial => 47,
            Self::MaterialDragonresource => 48,
            Self::MaterialDragonuniformmap => 49,
            Self::MaterialRendermaterial => 50,
            Self::MaterialRendermaterialgroup => 51,
            Self::MaterialVariationmanager => 52,
            Self::MoLang => 53,
            Self::Oreui => 54,
            Self::OreuiClient => 55,
            Self::PersonaPieces => 56,
            Self::PersonaAnimations => 57,
            Self::PersonaCharacters => 58,
            Self::PersonaSkinpacks => 59,
            Self::PersonaRepo => 60,
            Self::Player => 61,
            Self::Renderchunk => 62,
            Self::RenderchunkIndexbuffer => 63,
            Self::RenderchunkVertexbuffer => 64,
            Self::Rendering => 65,
            Self::RenderingBgfxinit => 66,
            Self::RenderingBgfxstartframe => 67,
            Self::RenderingBlocktessellator => 68,
            Self::RenderingEndframe => 69,
            Self::RenderingGraphicstasksinit => 70,
            Self::RenderingLibrary => 71,
            Self::RenderingPolygonoperatorpool => 72,
            Self::RenderingPbrtexturedata => 73,
            Self::RenderingRenderregistry => 74,
            Self::RenderingSetup => 75,
            Self::RenderingVertices => 76,
            Self::Requestlog => 77,
            Self::Resourcepacks => 78,
            Self::Sound => 79,
            Self::SubchunkBiomedata => 80,
            Self::SubchunkBlockdata => 81,
            Self::SubchunkLightdata => 82,
            Self::Textures => 83,
            Self::Weatherrenderer => 84,
            Self::WorldGenerator => 85,
            Self::Tasks => 86,
            Self::Test => 87,
            Self::TestLoadtesttags => 88,
            Self::Scripting => 89,
            Self::ScriptingRuntime => 90,
            Self::ScriptingContext => 91,
            Self::ScriptingContextBindingsMc => 92,
            Self::ScriptingContextBindingsGt => 93,
            Self::ScriptingContextRun => 94,
            Self::Datadrivenui => 95,
            Self::DatadrivenuiDefs => 96,
            Self::Gameface => 97,
            Self::GamefaceSystem => 98,
            Self::GamefaceDom => 99,
            Self::GamefaceCss => 100,
            Self::GamefaceDisplay => 101,
            Self::GamefaceTempallocator => 102,
            Self::GamefacePoolallocator => 103,
            Self::GamefaceDump => 104,
            Self::GamefaceMedia => 105,
            Self::GamefaceJson => 106,
            Self::GamefaceScriptengine => 107,
            Self::GamefaceScript => 108,
            Self::GamefaceLayout => 109,
            Self::Unknown2(value) => value,
        }
    }
}

impl From<MemoryCategory> for u8 {
    fn from(value: MemoryCategory) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for MemoryCategory {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for MemoryCategory {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

// Domain: misc

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum AgentActionType {
    #[default]
    Attack,
    Collect,
    Destroy,
    Detectredstone,
    Detectobstacle,
    Drop,
    Dropall,
    Inspect,
    Inspectdata,
    Inspectitemcount,
    Inspectitemdetail,
    Inspectitemspace,
    Interact,
    Move,
    Placeblock,
    Till,
    Transferitemto,
    Turn,
    Unknown(i32),
}

impl From<i32> for AgentActionType {
    fn from(value: i32) -> Self {
        match value {
            1 => Self::Attack,
            2 => Self::Collect,
            3 => Self::Destroy,
            4 => Self::Detectredstone,
            5 => Self::Detectobstacle,
            6 => Self::Drop,
            7 => Self::Dropall,
            8 => Self::Inspect,
            9 => Self::Inspectdata,
            10 => Self::Inspectitemcount,
            11 => Self::Inspectitemdetail,
            12 => Self::Inspectitemspace,
            13 => Self::Interact,
            14 => Self::Move,
            15 => Self::Placeblock,
            16 => Self::Till,
            17 => Self::Transferitemto,
            18 => Self::Turn,
            value => Self::Unknown(value),
        }
    }
}

impl AgentActionType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Attack => 1,
            Self::Collect => 2,
            Self::Destroy => 3,
            Self::Detectredstone => 4,
            Self::Detectobstacle => 5,
            Self::Drop => 6,
            Self::Dropall => 7,
            Self::Inspect => 8,
            Self::Inspectdata => 9,
            Self::Inspectitemcount => 10,
            Self::Inspectitemdetail => 11,
            Self::Inspectitemspace => 12,
            Self::Interact => 13,
            Self::Move => 14,
            Self::Placeblock => 15,
            Self::Till => 16,
            Self::Transferitemto => 17,
            Self::Turn => 18,
            Self::Unknown(value) => value,
        }
    }
}

impl From<AgentActionType> for i32 {
    fn from(value: AgentActionType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for AgentActionType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::I32LE(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for AgentActionType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::I32LE as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum AgentAnimationType {
    #[default]
    Armswing,
    Shrug,
    Unknown(u8),
}

impl From<u8> for AgentAnimationType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Armswing,
            1 => Self::Shrug,
            value => Self::Unknown(value),
        }
    }
}

impl AgentAnimationType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Armswing => 0,
            Self::Shrug => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<AgentAnimationType> for u8 {
    fn from(value: AgentAnimationType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for AgentAnimationType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for AgentAnimationType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum AnimateAction {
    #[default]
    Noaction,
    Swing,
    Wakeup,
    Criticalhit,
    Magiccriticalhit,
    Unknown(u8),
}

impl From<u8> for AnimateAction {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Noaction,
            1 => Self::Swing,
            3 => Self::Wakeup,
            4 => Self::Criticalhit,
            5 => Self::Magiccriticalhit,
            value => Self::Unknown(value),
        }
    }
}

impl AnimateAction {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Noaction => 0,
            Self::Swing => 1,
            Self::Wakeup => 3,
            Self::Criticalhit => 4,
            Self::Magiccriticalhit => 5,
            Self::Unknown(value) => value,
        }
    }
}

impl From<AnimateAction> for u8 {
    fn from(value: AnimateAction) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for AnimateAction {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for AnimateAction {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum AnimationMode {
    #[default]
    None,
    Layers,
    Blocks,
    Unknown(u8),
}

impl From<u8> for AnimationMode {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Layers,
            2 => Self::Blocks,
            value => Self::Unknown(value),
        }
    }
}

impl AnimationMode {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Layers => 1,
            Self::Blocks => 2,
            Self::Unknown(value) => value,
        }
    }
}

impl From<AnimationMode> for u8 {
    fn from(value: AnimationMode) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for AnimationMode {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for AnimationMode {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum BossBarColor {
    #[default]
    Pink,
    Blue,
    Red,
    Green,
    Yellow,
    Purple,
    RebeccaPurple,
    White,
    Unknown(u8),
}

impl From<u8> for BossBarColor {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Pink,
            1 => Self::Blue,
            2 => Self::Red,
            3 => Self::Green,
            4 => Self::Yellow,
            5 => Self::Purple,
            6 => Self::RebeccaPurple,
            7 => Self::White,
            value => Self::Unknown(value),
        }
    }
}

impl BossBarColor {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Pink => 0,
            Self::Blue => 1,
            Self::Red => 2,
            Self::Green => 3,
            Self::Yellow => 4,
            Self::Purple => 5,
            Self::RebeccaPurple => 6,
            Self::White => 7,
            Self::Unknown(value) => value,
        }
    }
}

impl From<BossBarColor> for u8 {
    fn from(value: BossBarColor) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for BossBarColor {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for BossBarColor {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum BossBarOverlay {
    #[default]
    Progress,
    Notched6,
    Notched10,
    Notched12,
    Notched20,
    Unknown(u8),
}

impl From<u8> for BossBarOverlay {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Progress,
            1 => Self::Notched6,
            2 => Self::Notched10,
            3 => Self::Notched12,
            4 => Self::Notched20,
            value => Self::Unknown(value),
        }
    }
}

impl BossBarOverlay {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Progress => 0,
            Self::Notched6 => 1,
            Self::Notched10 => 2,
            Self::Notched12 => 3,
            Self::Notched20 => 4,
            Self::Unknown(value) => value,
        }
    }
}

impl From<BossBarOverlay> for u8 {
    fn from(value: BossBarOverlay) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for BossBarOverlay {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for BossBarOverlay {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum BossEventUpdateType {
    #[default]
    Add,
    Playeradded,
    Remove,
    Playerremoved,
    UpdatePercent,
    UpdateName,
    UpdateProperties,
    UpdateStyle,
    Query,
    Unknown(u8),
}

impl From<u8> for BossEventUpdateType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Add,
            1 => Self::Playeradded,
            2 => Self::Remove,
            3 => Self::Playerremoved,
            4 => Self::UpdatePercent,
            5 => Self::UpdateName,
            6 => Self::UpdateProperties,
            7 => Self::UpdateStyle,
            8 => Self::Query,
            value => Self::Unknown(value),
        }
    }
}

impl BossEventUpdateType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Add => 0,
            Self::Playeradded => 1,
            Self::Remove => 2,
            Self::Playerremoved => 3,
            Self::UpdatePercent => 4,
            Self::UpdateName => 5,
            Self::UpdateProperties => 6,
            Self::UpdateStyle => 7,
            Self::Query => 8,
            Self::Unknown(value) => value,
        }
    }
}

impl From<BossEventUpdateType> for u8 {
    fn from(value: BossEventUpdateType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for BossEventUpdateType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for BossEventUpdateType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum BuildPlatform {
    #[default]
    Unknown,
    Google,
    Ios,
    Osx,
    Amazon,
    Gearvr,
    Uwp,
    Win32,
    Dedicated,
    Tvos,
    Sony,
    Nintendo,
    Xbox,
    Windowsphone,
    Linux,
    Unknown2(i32),
}

impl From<i32> for BuildPlatform {
    fn from(value: i32) -> Self {
        match value {
            -1 => Self::Unknown,
            1 => Self::Google,
            2 => Self::Ios,
            3 => Self::Osx,
            4 => Self::Amazon,
            5 => Self::Gearvr,
            7 => Self::Uwp,
            8 => Self::Win32,
            9 => Self::Dedicated,
            10 => Self::Tvos,
            11 => Self::Sony,
            12 => Self::Nintendo,
            13 => Self::Xbox,
            14 => Self::Windowsphone,
            15 => Self::Linux,
            value => Self::Unknown2(value),
        }
    }
}

impl BuildPlatform {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Unknown => -1,
            Self::Google => 1,
            Self::Ios => 2,
            Self::Osx => 3,
            Self::Amazon => 4,
            Self::Gearvr => 5,
            Self::Uwp => 7,
            Self::Win32 => 8,
            Self::Dedicated => 9,
            Self::Tvos => 10,
            Self::Sony => 11,
            Self::Nintendo => 12,
            Self::Xbox => 13,
            Self::Windowsphone => 14,
            Self::Linux => 15,
            Self::Unknown2(value) => value,
        }
    }
}

impl From<BuildPlatform> for i32 {
    fn from(value: BuildPlatform) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for BuildPlatform {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::I32LE(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for BuildPlatform {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::I32LE as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ChatRestrictionLevel {
    #[default]
    None,
    Dropped,
    Disabled,
    Unknown(u8),
}

impl From<u8> for ChatRestrictionLevel {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Dropped,
            2 => Self::Disabled,
            value => Self::Unknown(value),
        }
    }
}

impl ChatRestrictionLevel {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Dropped => 1,
            Self::Disabled => 2,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ChatRestrictionLevel> for u8 {
    fn from(value: ChatRestrictionLevel) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ChatRestrictionLevel {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ChatRestrictionLevel {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ClientCameraAimAssistAction {
    #[default]
    Setfromcamerapreset,
    Clear,
    Unknown(u8),
}

impl From<u8> for ClientCameraAimAssistAction {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Setfromcamerapreset,
            1 => Self::Clear,
            value => Self::Unknown(value),
        }
    }
}

impl ClientCameraAimAssistAction {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Setfromcamerapreset => 0,
            Self::Clear => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ClientCameraAimAssistAction> for u8 {
    fn from(value: ClientCameraAimAssistAction) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ClientCameraAimAssistAction {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ClientCameraAimAssistAction {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ClientPlayMode {
    #[default]
    Normal,
    Teaser,
    Screen,
    Viewer,
    Reality,
    Placement,
    Livingroom,
    Exitlevel,
    Exitlevellivingroom,
    Nummodes,
    Unknown(u32),
}

impl From<u32> for ClientPlayMode {
    fn from(value: u32) -> Self {
        match value {
            0 => Self::Normal,
            1 => Self::Teaser,
            2 => Self::Screen,
            3 => Self::Viewer,
            4 => Self::Reality,
            5 => Self::Placement,
            6 => Self::Livingroom,
            7 => Self::Exitlevel,
            8 => Self::Exitlevellivingroom,
            9 => Self::Nummodes,
            value => Self::Unknown(value),
        }
    }
}

impl ClientPlayMode {
    pub fn to_raw(self) -> u32 {
        match self {
            Self::Normal => 0,
            Self::Teaser => 1,
            Self::Screen => 2,
            Self::Viewer => 3,
            Self::Reality => 4,
            Self::Placement => 5,
            Self::Livingroom => 6,
            Self::Exitlevel => 7,
            Self::Exitlevellivingroom => 8,
            Self::Nummodes => 9,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ClientPlayMode> for u32 {
    fn from(value: ClientPlayMode) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ClientPlayMode {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::VarUInt(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ClientPlayMode {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::VarUInt as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ClientboundTextureShiftAction {
    #[default]
    Invalid,
    Initialize,
    Start,
    Setenabled,
    Sync,
    Unknown(u8),
}

impl From<u8> for ClientboundTextureShiftAction {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Invalid,
            1 => Self::Initialize,
            2 => Self::Start,
            3 => Self::Setenabled,
            4 => Self::Sync,
            value => Self::Unknown(value),
        }
    }
}

impl ClientboundTextureShiftAction {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Invalid => 0,
            Self::Initialize => 1,
            Self::Start => 2,
            Self::Setenabled => 3,
            Self::Sync => 4,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ClientboundTextureShiftAction> for u8 {
    fn from(value: ClientboundTextureShiftAction) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ClientboundTextureShiftAction {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ClientboundTextureShiftAction {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum CodeBuilderExecutionStateCodeStatus {
    #[default]
    None,
    Notstarted,
    Inprogress,
    Paused,
    Error,
    Succeeded,
    Unknown(u8),
}

impl From<u8> for CodeBuilderExecutionStateCodeStatus {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Notstarted,
            2 => Self::Inprogress,
            3 => Self::Paused,
            4 => Self::Error,
            5 => Self::Succeeded,
            value => Self::Unknown(value),
        }
    }
}

impl CodeBuilderExecutionStateCodeStatus {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Notstarted => 1,
            Self::Inprogress => 2,
            Self::Paused => 3,
            Self::Error => 4,
            Self::Succeeded => 5,
            Self::Unknown(value) => value,
        }
    }
}

impl From<CodeBuilderExecutionStateCodeStatus> for u8 {
    fn from(value: CodeBuilderExecutionStateCodeStatus) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for CodeBuilderExecutionStateCodeStatus {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CodeBuilderExecutionStateCodeStatus {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum CodeBuilderStorageQueryOptionsCategory {
    #[default]
    None,
    Codestatus,
    Instantiation,
    Unknown(u8),
}

impl From<u8> for CodeBuilderStorageQueryOptionsCategory {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Codestatus,
            2 => Self::Instantiation,
            value => Self::Unknown(value),
        }
    }
}

impl CodeBuilderStorageQueryOptionsCategory {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Codestatus => 1,
            Self::Instantiation => 2,
            Self::Unknown(value) => value,
        }
    }
}

impl From<CodeBuilderStorageQueryOptionsCategory> for u8 {
    fn from(value: CodeBuilderStorageQueryOptionsCategory) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for CodeBuilderStorageQueryOptionsCategory {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CodeBuilderStorageQueryOptionsCategory {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum CodeBuilderStorageQueryOptionsOperation {
    #[default]
    None,
    Get,
    Set,
    Reset,
    Unknown(u8),
}

impl From<u8> for CodeBuilderStorageQueryOptionsOperation {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Get,
            2 => Self::Set,
            3 => Self::Reset,
            value => Self::Unknown(value),
        }
    }
}

impl CodeBuilderStorageQueryOptionsOperation {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Get => 1,
            Self::Set => 2,
            Self::Reset => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<CodeBuilderStorageQueryOptionsOperation> for u8 {
    fn from(value: CodeBuilderStorageQueryOptionsOperation) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for CodeBuilderStorageQueryOptionsOperation {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CodeBuilderStorageQueryOptionsOperation {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ConnectionDisconnectFailReason {
    #[default]
    Unknown,
    Cantconnectnointernet,
    Nopermissions,
    Unrecoverableerror,
    Thirdpartyblocked,
    Thirdpartynointernet,
    Thirdpartybadip,
    Thirdpartynoserverorserverlocked,
    Versionmismatch,
    Skinissue,
    Invitesessionnotfound,
    Edulevelsettingsmissing,
    Localservernotfound,
    Legacydisconnect,
    InternalUserleavegameattempted,
    Platformlockedskinserror,
    Realmsworldunassigned,
    Realmsservercantconnect,
    Realmsserverhidden,
    Realmsserverdisabledbeta,
    Realmsserverdisabled,
    Crossplatformdisabled,
    TestonlyCantconnect,
    Sessionnotfound,
    Clientsettingsincompatiblewithserver,
    Serverfull,
    Invalidplatformskin,
    Editionversionmismatch,
    Editionmismatch,
    Levelnewerthanexeversion,
    InternalNofailoccurred,
    Bannedskin,
    Timeout,
    Servernotfound,
    Outdatedserver,
    Outdatedclient,
    Nopremiumplatform,
    Multiplayerdisabled,
    Nowifi,
    Worldcorruption,
    Noreason,
    Disconnected,
    Invalidplayer,
    Loggedinotherlocation,
    Serveridconflict,
    Notallowed,
    Notauthenticated,
    Invalidtenant,
    Unknownpacket,
    Unexpectedpacket,
    Invalidcommandrequestpacket,
    Hostsuspended,
    Loginpacketnorequest,
    Loginpacketnocert,
    Missingclient,
    Kicked,
    Kickedforexploit,
    Kickedforidle,
    Resourcepackproblem,
    Incompatiblepack,
    Outofstorage,
    Invalidlevel,
    Disconnectpacket,
    Blockmismatch,
    Invalidheights,
    Invalidwidths,
    Connectionlost,
    Zombieconnection,
    Shutdown,
    Reasonnotset,
    Loadingstatetimeout,
    Resourcepackloadingfailed,
    Searchingforsessionloadingscreenfailed,
    Nethernetprotocolversion,
    Subsystemstatuserror,
    Emptyauthfromdiscovery,
    Emptyurlfromdiscovery,
    Expiredauthfromdiscovery,
    Unknownsignalservicesigninfailure,
    Xbljoinlobbyfailure,
    Unspecifiedclientinstancedisconnection,
    Nethernetsessionnotfound,
    Nethernetcreatepeerconnection,
    Nethernetice,
    Nethernetconnectrequest,
    Nethernetconnectresponse,
    Nethernetnegotiationtimeout,
    Nethernetinactivitytimeout,
    Staleconnectionbeingreplaced,
    Realmssessionnotfound,
    Badpacket,
    Nethernetfailedtocreateoffer,
    Nethernetfailedtocreateanswer,
    Nethernetfailedtosetlocaldescription,
    Nethernetfailedtosetremotedescription,
    Nethernetnegotiationtimeoutwaitingforresponse,
    Nethernetnegotiationtimeoutwaitingforaccept,
    Nethernetincomingconnectionignored,
    Nethernetsignalingparsingfailure,
    Nethernetsignalingunknownerror,
    Nethernetsignalingunicastdeliveryfailed,
    Nethernetsignalingbroadcastdeliveryfailed,
    Nethernetsignalinggenericdeliveryfailed,
    Editormismatcheditorworld,
    Editormismatchvanillaworld,
    Worldtransfernotprimaryclient,
    InternalRequestservershutdown,
    Clientgamesetupcancelled,
    Clientgamesetupfailed,
    Novenue,
    Nethernetsignalingsigninfailed,
    Sessionaccessdenied,
    Servicesigninissue,
    Nethernetnosignalingchannel,
    Nethernetnotloggedin,
    Nethernetclientsignalingerror,
    Subclientlogindisabled,
    Deeplinktryingtoopendemoworldwhilesignedin,
    Asyncjointaskdenied,
    Realmstimelinerequired,
    Guestwithouthost,
    Failedtojoinexperience,
    Nethernetdatachannelclosed,
    Discoveryenvironmentmismatch,
    Hostwithoutkeys,
    Hostsignedout,
    Scriptwatchdogexception,
    Scriptmemorylimitexceeded,
    Storagelowduringgameplay,
    Storagefullduringgameplay,
    Levelstoragecorruption,
    Editionmismatchvanillatoedu,
    Editionmismatchedutovanilla,
    Editormismatcheditortovanilla,
    Editormismatchvanillatoeditor,
    Denylisted,
    Noncemissing,
    Noncenotfound,
    Nonceexpired,
    Noncenotvalid,
    Hostdisconnected,
    Editorjoinintentpolicyfailure,
    Nethernetidentitynotallowed,
    Invalidname,
    Expiredtoken,
    Hostacceptsnotypeofauth,
    Notauthenticatedfastfail,
    Editornotallowed,
    Missingstructuredata,
    Unsupportedtransport,
    Unknown2(i32),
}

impl From<i32> for ConnectionDisconnectFailReason {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Unknown,
            1 => Self::Cantconnectnointernet,
            2 => Self::Nopermissions,
            3 => Self::Unrecoverableerror,
            4 => Self::Thirdpartyblocked,
            5 => Self::Thirdpartynointernet,
            6 => Self::Thirdpartybadip,
            7 => Self::Thirdpartynoserverorserverlocked,
            8 => Self::Versionmismatch,
            9 => Self::Skinissue,
            10 => Self::Invitesessionnotfound,
            11 => Self::Edulevelsettingsmissing,
            12 => Self::Localservernotfound,
            13 => Self::Legacydisconnect,
            14 => Self::InternalUserleavegameattempted,
            15 => Self::Platformlockedskinserror,
            16 => Self::Realmsworldunassigned,
            17 => Self::Realmsservercantconnect,
            18 => Self::Realmsserverhidden,
            19 => Self::Realmsserverdisabledbeta,
            20 => Self::Realmsserverdisabled,
            21 => Self::Crossplatformdisabled,
            22 => Self::TestonlyCantconnect,
            23 => Self::Sessionnotfound,
            24 => Self::Clientsettingsincompatiblewithserver,
            25 => Self::Serverfull,
            26 => Self::Invalidplatformskin,
            27 => Self::Editionversionmismatch,
            28 => Self::Editionmismatch,
            29 => Self::Levelnewerthanexeversion,
            30 => Self::InternalNofailoccurred,
            31 => Self::Bannedskin,
            32 => Self::Timeout,
            33 => Self::Servernotfound,
            34 => Self::Outdatedserver,
            35 => Self::Outdatedclient,
            36 => Self::Nopremiumplatform,
            37 => Self::Multiplayerdisabled,
            38 => Self::Nowifi,
            39 => Self::Worldcorruption,
            40 => Self::Noreason,
            41 => Self::Disconnected,
            42 => Self::Invalidplayer,
            43 => Self::Loggedinotherlocation,
            44 => Self::Serveridconflict,
            45 => Self::Notallowed,
            46 => Self::Notauthenticated,
            47 => Self::Invalidtenant,
            48 => Self::Unknownpacket,
            49 => Self::Unexpectedpacket,
            50 => Self::Invalidcommandrequestpacket,
            51 => Self::Hostsuspended,
            52 => Self::Loginpacketnorequest,
            53 => Self::Loginpacketnocert,
            54 => Self::Missingclient,
            55 => Self::Kicked,
            56 => Self::Kickedforexploit,
            57 => Self::Kickedforidle,
            58 => Self::Resourcepackproblem,
            59 => Self::Incompatiblepack,
            60 => Self::Outofstorage,
            61 => Self::Invalidlevel,
            62 => Self::Disconnectpacket,
            63 => Self::Blockmismatch,
            64 => Self::Invalidheights,
            65 => Self::Invalidwidths,
            66 => Self::Connectionlost,
            67 => Self::Zombieconnection,
            68 => Self::Shutdown,
            69 => Self::Reasonnotset,
            70 => Self::Loadingstatetimeout,
            71 => Self::Resourcepackloadingfailed,
            72 => Self::Searchingforsessionloadingscreenfailed,
            73 => Self::Nethernetprotocolversion,
            74 => Self::Subsystemstatuserror,
            75 => Self::Emptyauthfromdiscovery,
            76 => Self::Emptyurlfromdiscovery,
            77 => Self::Expiredauthfromdiscovery,
            78 => Self::Unknownsignalservicesigninfailure,
            79 => Self::Xbljoinlobbyfailure,
            80 => Self::Unspecifiedclientinstancedisconnection,
            81 => Self::Nethernetsessionnotfound,
            82 => Self::Nethernetcreatepeerconnection,
            83 => Self::Nethernetice,
            84 => Self::Nethernetconnectrequest,
            85 => Self::Nethernetconnectresponse,
            86 => Self::Nethernetnegotiationtimeout,
            87 => Self::Nethernetinactivitytimeout,
            88 => Self::Staleconnectionbeingreplaced,
            89 => Self::Realmssessionnotfound,
            90 => Self::Badpacket,
            91 => Self::Nethernetfailedtocreateoffer,
            92 => Self::Nethernetfailedtocreateanswer,
            93 => Self::Nethernetfailedtosetlocaldescription,
            94 => Self::Nethernetfailedtosetremotedescription,
            95 => Self::Nethernetnegotiationtimeoutwaitingforresponse,
            96 => Self::Nethernetnegotiationtimeoutwaitingforaccept,
            97 => Self::Nethernetincomingconnectionignored,
            98 => Self::Nethernetsignalingparsingfailure,
            99 => Self::Nethernetsignalingunknownerror,
            100 => Self::Nethernetsignalingunicastdeliveryfailed,
            101 => Self::Nethernetsignalingbroadcastdeliveryfailed,
            102 => Self::Nethernetsignalinggenericdeliveryfailed,
            103 => Self::Editormismatcheditorworld,
            104 => Self::Editormismatchvanillaworld,
            105 => Self::Worldtransfernotprimaryclient,
            106 => Self::InternalRequestservershutdown,
            107 => Self::Clientgamesetupcancelled,
            108 => Self::Clientgamesetupfailed,
            109 => Self::Novenue,
            110 => Self::Nethernetsignalingsigninfailed,
            111 => Self::Sessionaccessdenied,
            112 => Self::Servicesigninissue,
            113 => Self::Nethernetnosignalingchannel,
            114 => Self::Nethernetnotloggedin,
            115 => Self::Nethernetclientsignalingerror,
            116 => Self::Subclientlogindisabled,
            117 => Self::Deeplinktryingtoopendemoworldwhilesignedin,
            118 => Self::Asyncjointaskdenied,
            119 => Self::Realmstimelinerequired,
            120 => Self::Guestwithouthost,
            121 => Self::Failedtojoinexperience,
            122 => Self::Nethernetdatachannelclosed,
            123 => Self::Discoveryenvironmentmismatch,
            124 => Self::Hostwithoutkeys,
            125 => Self::Hostsignedout,
            126 => Self::Scriptwatchdogexception,
            127 => Self::Scriptmemorylimitexceeded,
            128 => Self::Storagelowduringgameplay,
            129 => Self::Storagefullduringgameplay,
            130 => Self::Levelstoragecorruption,
            131 => Self::Editionmismatchvanillatoedu,
            132 => Self::Editionmismatchedutovanilla,
            133 => Self::Editormismatcheditortovanilla,
            134 => Self::Editormismatchvanillatoeditor,
            135 => Self::Denylisted,
            136 => Self::Noncemissing,
            137 => Self::Noncenotfound,
            138 => Self::Nonceexpired,
            139 => Self::Noncenotvalid,
            140 => Self::Hostdisconnected,
            141 => Self::Editorjoinintentpolicyfailure,
            142 => Self::Nethernetidentitynotallowed,
            143 => Self::Invalidname,
            144 => Self::Expiredtoken,
            145 => Self::Hostacceptsnotypeofauth,
            146 => Self::Notauthenticatedfastfail,
            147 => Self::Editornotallowed,
            148 => Self::Missingstructuredata,
            149 => Self::Unsupportedtransport,
            value => Self::Unknown2(value),
        }
    }
}

impl ConnectionDisconnectFailReason {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Unknown => 0,
            Self::Cantconnectnointernet => 1,
            Self::Nopermissions => 2,
            Self::Unrecoverableerror => 3,
            Self::Thirdpartyblocked => 4,
            Self::Thirdpartynointernet => 5,
            Self::Thirdpartybadip => 6,
            Self::Thirdpartynoserverorserverlocked => 7,
            Self::Versionmismatch => 8,
            Self::Skinissue => 9,
            Self::Invitesessionnotfound => 10,
            Self::Edulevelsettingsmissing => 11,
            Self::Localservernotfound => 12,
            Self::Legacydisconnect => 13,
            Self::InternalUserleavegameattempted => 14,
            Self::Platformlockedskinserror => 15,
            Self::Realmsworldunassigned => 16,
            Self::Realmsservercantconnect => 17,
            Self::Realmsserverhidden => 18,
            Self::Realmsserverdisabledbeta => 19,
            Self::Realmsserverdisabled => 20,
            Self::Crossplatformdisabled => 21,
            Self::TestonlyCantconnect => 22,
            Self::Sessionnotfound => 23,
            Self::Clientsettingsincompatiblewithserver => 24,
            Self::Serverfull => 25,
            Self::Invalidplatformskin => 26,
            Self::Editionversionmismatch => 27,
            Self::Editionmismatch => 28,
            Self::Levelnewerthanexeversion => 29,
            Self::InternalNofailoccurred => 30,
            Self::Bannedskin => 31,
            Self::Timeout => 32,
            Self::Servernotfound => 33,
            Self::Outdatedserver => 34,
            Self::Outdatedclient => 35,
            Self::Nopremiumplatform => 36,
            Self::Multiplayerdisabled => 37,
            Self::Nowifi => 38,
            Self::Worldcorruption => 39,
            Self::Noreason => 40,
            Self::Disconnected => 41,
            Self::Invalidplayer => 42,
            Self::Loggedinotherlocation => 43,
            Self::Serveridconflict => 44,
            Self::Notallowed => 45,
            Self::Notauthenticated => 46,
            Self::Invalidtenant => 47,
            Self::Unknownpacket => 48,
            Self::Unexpectedpacket => 49,
            Self::Invalidcommandrequestpacket => 50,
            Self::Hostsuspended => 51,
            Self::Loginpacketnorequest => 52,
            Self::Loginpacketnocert => 53,
            Self::Missingclient => 54,
            Self::Kicked => 55,
            Self::Kickedforexploit => 56,
            Self::Kickedforidle => 57,
            Self::Resourcepackproblem => 58,
            Self::Incompatiblepack => 59,
            Self::Outofstorage => 60,
            Self::Invalidlevel => 61,
            Self::Disconnectpacket => 62,
            Self::Blockmismatch => 63,
            Self::Invalidheights => 64,
            Self::Invalidwidths => 65,
            Self::Connectionlost => 66,
            Self::Zombieconnection => 67,
            Self::Shutdown => 68,
            Self::Reasonnotset => 69,
            Self::Loadingstatetimeout => 70,
            Self::Resourcepackloadingfailed => 71,
            Self::Searchingforsessionloadingscreenfailed => 72,
            Self::Nethernetprotocolversion => 73,
            Self::Subsystemstatuserror => 74,
            Self::Emptyauthfromdiscovery => 75,
            Self::Emptyurlfromdiscovery => 76,
            Self::Expiredauthfromdiscovery => 77,
            Self::Unknownsignalservicesigninfailure => 78,
            Self::Xbljoinlobbyfailure => 79,
            Self::Unspecifiedclientinstancedisconnection => 80,
            Self::Nethernetsessionnotfound => 81,
            Self::Nethernetcreatepeerconnection => 82,
            Self::Nethernetice => 83,
            Self::Nethernetconnectrequest => 84,
            Self::Nethernetconnectresponse => 85,
            Self::Nethernetnegotiationtimeout => 86,
            Self::Nethernetinactivitytimeout => 87,
            Self::Staleconnectionbeingreplaced => 88,
            Self::Realmssessionnotfound => 89,
            Self::Badpacket => 90,
            Self::Nethernetfailedtocreateoffer => 91,
            Self::Nethernetfailedtocreateanswer => 92,
            Self::Nethernetfailedtosetlocaldescription => 93,
            Self::Nethernetfailedtosetremotedescription => 94,
            Self::Nethernetnegotiationtimeoutwaitingforresponse => 95,
            Self::Nethernetnegotiationtimeoutwaitingforaccept => 96,
            Self::Nethernetincomingconnectionignored => 97,
            Self::Nethernetsignalingparsingfailure => 98,
            Self::Nethernetsignalingunknownerror => 99,
            Self::Nethernetsignalingunicastdeliveryfailed => 100,
            Self::Nethernetsignalingbroadcastdeliveryfailed => 101,
            Self::Nethernetsignalinggenericdeliveryfailed => 102,
            Self::Editormismatcheditorworld => 103,
            Self::Editormismatchvanillaworld => 104,
            Self::Worldtransfernotprimaryclient => 105,
            Self::InternalRequestservershutdown => 106,
            Self::Clientgamesetupcancelled => 107,
            Self::Clientgamesetupfailed => 108,
            Self::Novenue => 109,
            Self::Nethernetsignalingsigninfailed => 110,
            Self::Sessionaccessdenied => 111,
            Self::Servicesigninissue => 112,
            Self::Nethernetnosignalingchannel => 113,
            Self::Nethernetnotloggedin => 114,
            Self::Nethernetclientsignalingerror => 115,
            Self::Subclientlogindisabled => 116,
            Self::Deeplinktryingtoopendemoworldwhilesignedin => 117,
            Self::Asyncjointaskdenied => 118,
            Self::Realmstimelinerequired => 119,
            Self::Guestwithouthost => 120,
            Self::Failedtojoinexperience => 121,
            Self::Nethernetdatachannelclosed => 122,
            Self::Discoveryenvironmentmismatch => 123,
            Self::Hostwithoutkeys => 124,
            Self::Hostsignedout => 125,
            Self::Scriptwatchdogexception => 126,
            Self::Scriptmemorylimitexceeded => 127,
            Self::Storagelowduringgameplay => 128,
            Self::Storagefullduringgameplay => 129,
            Self::Levelstoragecorruption => 130,
            Self::Editionmismatchvanillatoedu => 131,
            Self::Editionmismatchedutovanilla => 132,
            Self::Editormismatcheditortovanilla => 133,
            Self::Editormismatchvanillatoeditor => 134,
            Self::Denylisted => 135,
            Self::Noncemissing => 136,
            Self::Noncenotfound => 137,
            Self::Nonceexpired => 138,
            Self::Noncenotvalid => 139,
            Self::Hostdisconnected => 140,
            Self::Editorjoinintentpolicyfailure => 141,
            Self::Nethernetidentitynotallowed => 142,
            Self::Invalidname => 143,
            Self::Expiredtoken => 144,
            Self::Hostacceptsnotypeofauth => 145,
            Self::Notauthenticatedfastfail => 146,
            Self::Editornotallowed => 147,
            Self::Missingstructuredata => 148,
            Self::Unsupportedtransport => 149,
            Self::Unknown2(value) => value,
        }
    }
}

impl From<ConnectionDisconnectFailReason> for i32 {
    fn from(value: ConnectionDisconnectFailReason) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ConnectionDisconnectFailReason {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ConnectionDisconnectFailReason {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ControlScheme {
    #[default]
    LockedPlayerRelativeStrafe,
    CameraRelative,
    CameraRelativeStrafe,
    PlayerRelative,
    PlayerRelativeStrafe,
    Unknown(u8),
}

impl From<u8> for ControlScheme {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::LockedPlayerRelativeStrafe,
            1 => Self::CameraRelative,
            2 => Self::CameraRelativeStrafe,
            3 => Self::PlayerRelative,
            4 => Self::PlayerRelativeStrafe,
            value => Self::Unknown(value),
        }
    }
}

impl ControlScheme {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::LockedPlayerRelativeStrafe => 0,
            Self::CameraRelative => 1,
            Self::CameraRelativeStrafe => 2,
            Self::PlayerRelative => 3,
            Self::PlayerRelativeStrafe => 4,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ControlScheme> for u8 {
    fn from(value: ControlScheme) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ControlScheme {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ControlScheme {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum CoordinateEvaluationOrder {
    #[default]
    Xyz,
    Xzy,
    Yxz,
    Yzx,
    Zxy,
    Zyx,
    Unknown(i32),
}

impl From<i32> for CoordinateEvaluationOrder {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Xyz,
            1 => Self::Xzy,
            2 => Self::Yxz,
            3 => Self::Yzx,
            4 => Self::Zxy,
            5 => Self::Zyx,
            value => Self::Unknown(value),
        }
    }
}

impl CoordinateEvaluationOrder {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Xyz => 0,
            Self::Xzy => 1,
            Self::Yxz => 2,
            Self::Yzx => 3,
            Self::Zxy => 4,
            Self::Zyx => 5,
            Self::Unknown(value) => value,
        }
    }
}

impl From<CoordinateEvaluationOrder> for i32 {
    fn from(value: CoordinateEvaluationOrder) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for CoordinateEvaluationOrder {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CoordinateEvaluationOrder {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum DataItemType {
    #[default]
    Byte,
    Short,
    Int,
    Float,
    String,
    Compoundtag,
    Pos,
    Int64,
    Vec3,
    Unknown(u8),
}

impl From<u8> for DataItemType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Byte,
            1 => Self::Short,
            2 => Self::Int,
            3 => Self::Float,
            4 => Self::String,
            5 => Self::Compoundtag,
            6 => Self::Pos,
            7 => Self::Int64,
            8 => Self::Vec3,
            value => Self::Unknown(value),
        }
    }
}

impl DataItemType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Byte => 0,
            Self::Short => 1,
            Self::Int => 2,
            Self::Float => 3,
            Self::String => 4,
            Self::Compoundtag => 5,
            Self::Pos => 6,
            Self::Int64 => 7,
            Self::Vec3 => 8,
            Self::Unknown(value) => value,
        }
    }
}

impl From<DataItemType> for u8 {
    fn from(value: DataItemType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for DataItemType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for DataItemType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum EditorWorldType {
    #[default]
    Noneditor,
    Editorproject,
    Editortestlevel,
    Editorrealmsupload,
    Unknown(i32),
}

impl From<i32> for EditorWorldType {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Noneditor,
            1 => Self::Editorproject,
            2 => Self::Editortestlevel,
            3 => Self::Editorrealmsupload,
            value => Self::Unknown(value),
        }
    }
}

impl EditorWorldType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Noneditor => 0,
            Self::Editorproject => 1,
            Self::Editortestlevel => 2,
            Self::Editorrealmsupload => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<EditorWorldType> for i32 {
    fn from(value: EditorWorldType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for EditorWorldType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for EditorWorldType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum GameType {
    #[default]
    Undefined,
    Survival,
    Creative,
    Adventure,
    Default,
    Spectator,
    Unknown(i32),
}

impl From<i32> for GameType {
    fn from(value: i32) -> Self {
        match value {
            -1 => Self::Undefined,
            0 => Self::Survival,
            1 => Self::Creative,
            2 => Self::Adventure,
            5 => Self::Default,
            6 => Self::Spectator,
            value => Self::Unknown(value),
        }
    }
}

impl GameType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Undefined => -1,
            Self::Survival => 0,
            Self::Creative => 1,
            Self::Adventure => 2,
            Self::Default => 5,
            Self::Spectator => 6,
            Self::Unknown(value) => value,
        }
    }
}

impl From<GameType> for i32 {
    fn from(value: GameType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for GameType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for GameType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum GeneratorType {
    #[default]
    Legacy,
    Overworld,
    Flat,
    Nether,
    Theend,
    Void,
    Undefined,
    Unknown(i32),
}

impl From<i32> for GeneratorType {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Legacy,
            1 => Self::Overworld,
            2 => Self::Flat,
            3 => Self::Nether,
            4 => Self::Theend,
            5 => Self::Void,
            6 => Self::Undefined,
            value => Self::Unknown(value),
        }
    }
}

impl GeneratorType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Legacy => 0,
            Self::Overworld => 1,
            Self::Flat => 2,
            Self::Nether => 3,
            Self::Theend => 4,
            Self::Void => 5,
            Self::Undefined => 6,
            Self::Unknown(value) => value,
        }
    }
}

impl From<GeneratorType> for i32 {
    fn from(value: GeneratorType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for GeneratorType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for GeneratorType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum GraphicsMode {
    #[default]
    Simple,
    Fancy,
    Advanced,
    Raytraced,
    Unknown(u8),
}

impl From<u8> for GraphicsMode {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Simple,
            1 => Self::Fancy,
            2 => Self::Advanced,
            3 => Self::Raytraced,
            value => Self::Unknown(value),
        }
    }
}

impl GraphicsMode {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Simple => 0,
            Self::Fancy => 1,
            Self::Advanced => 2,
            Self::Raytraced => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<GraphicsMode> for u8 {
    fn from(value: GraphicsMode) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for GraphicsMode {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for GraphicsMode {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum GraphicsOverrideParameterType {
    #[default]
    Skyzenithcolor,
    Skyhorizoncolor,
    Horizonblendmin,
    Horizonblendmax,
    Horizonblendstart,
    Horizonblendmiestart,
    Rayleighstrength,
    Sunmiestrength,
    Moonmiestrength,
    Sunglareshape,
    Chlorophyll,
    Cdom,
    Suspendedsediment,
    Wavesdepth,
    Wavesfrequency,
    Wavesfrequencyscaling,
    Wavesspeed,
    Wavesspeedscaling,
    Wavesshape,
    Wavesoctaves,
    Wavesmix,
    Wavespull,
    Wavesdirectionincrement,
    Midtonescontrast,
    Highlightscontrast,
    Shadowscontrast,
    Highlightsgain,
    Highlightsgamma,
    Highlightsoffset,
    Highlightssaturation,
    Midtonesgain,
    Midtonesgamma,
    Midtonesoffset,
    Midtonessaturation,
    Shadowsgain,
    Shadowsgamma,
    Shadowsoffset,
    Shadowssaturation,
    Highlightsmin,
    Shadowsmax,
    Temperature,
    Suncolor,
    Sunilluminance,
    Mooncolor,
    Moonilluminance,
    Flashcolor,
    Flashilluminance,
    Ambientcolor,
    Ambientilluminance,
    Emissivedesaturation,
    Skyintensity,
    Orbitaloffsetdegrees,
    Unknown(u8),
}

impl From<u8> for GraphicsOverrideParameterType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Skyzenithcolor,
            1 => Self::Skyhorizoncolor,
            2 => Self::Horizonblendmin,
            3 => Self::Horizonblendmax,
            4 => Self::Horizonblendstart,
            5 => Self::Horizonblendmiestart,
            6 => Self::Rayleighstrength,
            7 => Self::Sunmiestrength,
            8 => Self::Moonmiestrength,
            9 => Self::Sunglareshape,
            10 => Self::Chlorophyll,
            11 => Self::Cdom,
            12 => Self::Suspendedsediment,
            13 => Self::Wavesdepth,
            14 => Self::Wavesfrequency,
            15 => Self::Wavesfrequencyscaling,
            16 => Self::Wavesspeed,
            17 => Self::Wavesspeedscaling,
            18 => Self::Wavesshape,
            19 => Self::Wavesoctaves,
            20 => Self::Wavesmix,
            21 => Self::Wavespull,
            22 => Self::Wavesdirectionincrement,
            23 => Self::Midtonescontrast,
            24 => Self::Highlightscontrast,
            25 => Self::Shadowscontrast,
            26 => Self::Highlightsgain,
            27 => Self::Highlightsgamma,
            28 => Self::Highlightsoffset,
            29 => Self::Highlightssaturation,
            30 => Self::Midtonesgain,
            31 => Self::Midtonesgamma,
            32 => Self::Midtonesoffset,
            33 => Self::Midtonessaturation,
            34 => Self::Shadowsgain,
            35 => Self::Shadowsgamma,
            36 => Self::Shadowsoffset,
            37 => Self::Shadowssaturation,
            38 => Self::Highlightsmin,
            39 => Self::Shadowsmax,
            40 => Self::Temperature,
            41 => Self::Suncolor,
            42 => Self::Sunilluminance,
            43 => Self::Mooncolor,
            44 => Self::Moonilluminance,
            45 => Self::Flashcolor,
            46 => Self::Flashilluminance,
            47 => Self::Ambientcolor,
            48 => Self::Ambientilluminance,
            49 => Self::Emissivedesaturation,
            50 => Self::Skyintensity,
            51 => Self::Orbitaloffsetdegrees,
            value => Self::Unknown(value),
        }
    }
}

impl GraphicsOverrideParameterType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Skyzenithcolor => 0,
            Self::Skyhorizoncolor => 1,
            Self::Horizonblendmin => 2,
            Self::Horizonblendmax => 3,
            Self::Horizonblendstart => 4,
            Self::Horizonblendmiestart => 5,
            Self::Rayleighstrength => 6,
            Self::Sunmiestrength => 7,
            Self::Moonmiestrength => 8,
            Self::Sunglareshape => 9,
            Self::Chlorophyll => 10,
            Self::Cdom => 11,
            Self::Suspendedsediment => 12,
            Self::Wavesdepth => 13,
            Self::Wavesfrequency => 14,
            Self::Wavesfrequencyscaling => 15,
            Self::Wavesspeed => 16,
            Self::Wavesspeedscaling => 17,
            Self::Wavesshape => 18,
            Self::Wavesoctaves => 19,
            Self::Wavesmix => 20,
            Self::Wavespull => 21,
            Self::Wavesdirectionincrement => 22,
            Self::Midtonescontrast => 23,
            Self::Highlightscontrast => 24,
            Self::Shadowscontrast => 25,
            Self::Highlightsgain => 26,
            Self::Highlightsgamma => 27,
            Self::Highlightsoffset => 28,
            Self::Highlightssaturation => 29,
            Self::Midtonesgain => 30,
            Self::Midtonesgamma => 31,
            Self::Midtonesoffset => 32,
            Self::Midtonessaturation => 33,
            Self::Shadowsgain => 34,
            Self::Shadowsgamma => 35,
            Self::Shadowsoffset => 36,
            Self::Shadowssaturation => 37,
            Self::Highlightsmin => 38,
            Self::Shadowsmax => 39,
            Self::Temperature => 40,
            Self::Suncolor => 41,
            Self::Sunilluminance => 42,
            Self::Mooncolor => 43,
            Self::Moonilluminance => 44,
            Self::Flashcolor => 45,
            Self::Flashilluminance => 46,
            Self::Ambientcolor => 47,
            Self::Ambientilluminance => 48,
            Self::Emissivedesaturation => 49,
            Self::Skyintensity => 50,
            Self::Orbitaloffsetdegrees => 51,
            Self::Unknown(value) => value,
        }
    }
}

impl From<GraphicsOverrideParameterType> for u8 {
    fn from(value: GraphicsOverrideParameterType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for GraphicsOverrideParameterType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for GraphicsOverrideParameterType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum HeightMapDataType {
    #[default]
    Nodata,
    Hasdata,
    Alltoohigh,
    Alltoolow,
    Allcopied,
    Unknown(u8),
}

impl From<u8> for HeightMapDataType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Nodata,
            1 => Self::Hasdata,
            2 => Self::Alltoohigh,
            3 => Self::Alltoolow,
            4 => Self::Allcopied,
            value => Self::Unknown(value),
        }
    }
}

impl HeightMapDataType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Nodata => 0,
            Self::Hasdata => 1,
            Self::Alltoohigh => 2,
            Self::Alltoolow => 3,
            Self::Allcopied => 4,
            Self::Unknown(value) => value,
        }
    }
}

impl From<HeightMapDataType> for u8 {
    fn from(value: HeightMapDataType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for HeightMapDataType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for HeightMapDataType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum HudElement {
    #[default]
    Paperdoll,
    Armor,
    Tooltips,
    Touchcontrols,
    Crosshair,
    Hotbar,
    Health,
    Progressbar,
    Hunger,
    Airbubbles,
    Horsehealth,
    Statuseffects,
    Itemtext,
    Unknown(i32),
}

impl From<i32> for HudElement {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Paperdoll,
            1 => Self::Armor,
            2 => Self::Tooltips,
            3 => Self::Touchcontrols,
            4 => Self::Crosshair,
            5 => Self::Hotbar,
            6 => Self::Health,
            7 => Self::Progressbar,
            8 => Self::Hunger,
            9 => Self::Airbubbles,
            10 => Self::Horsehealth,
            11 => Self::Statuseffects,
            12 => Self::Itemtext,
            value => Self::Unknown(value),
        }
    }
}

impl HudElement {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Paperdoll => 0,
            Self::Armor => 1,
            Self::Tooltips => 2,
            Self::Touchcontrols => 3,
            Self::Crosshair => 4,
            Self::Hotbar => 5,
            Self::Health => 6,
            Self::Progressbar => 7,
            Self::Hunger => 8,
            Self::Airbubbles => 9,
            Self::Horsehealth => 10,
            Self::Statuseffects => 11,
            Self::Itemtext => 12,
            Self::Unknown(value) => value,
        }
    }
}

impl From<HudElement> for i32 {
    fn from(value: HudElement) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for HudElement {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for HudElement {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum HudVisibility {
    #[default]
    Hide,
    Reset,
    Unknown(i32),
}

impl From<i32> for HudVisibility {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Hide,
            1 => Self::Reset,
            value => Self::Unknown(value),
        }
    }
}

impl HudVisibility {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Hide => 0,
            Self::Reset => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<HudVisibility> for i32 {
    fn from(value: HudVisibility) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for HudVisibility {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for HudVisibility {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum InputData {
    #[default]
    Ascend,
    Descend,
    Northjump,
    Jumpdown,
    Sprintdown,
    Changeheight,
    Jumping,
    Autojumpinginwater,
    Sneaking,
    Sneakdown,
    Up,
    Down,
    Left,
    Right,
    Upleft,
    Upright,
    Wantup,
    Wantdown,
    Wantdownslow,
    Wantupslow,
    Sprinting,
    Ascendblock,
    Descendblock,
    Sneaktoggledown,
    Persistsneak,
    Startsprinting,
    Stopsprinting,
    Startsneaking,
    Stopsneaking,
    Startswimming,
    Stopswimming,
    Startjumping,
    Startgliding,
    Stopgliding,
    Performiteminteraction,
    Performblockactions,
    Performitemstackrequest,
    Handledteleport,
    Emoting,
    Missedswing,
    Startcrawling,
    Stopcrawling,
    Startflying,
    Stopflying,
    Clientackserverdata,
    Isinclientpredictedvehicle,
    Paddlingleft,
    Paddlingright,
    Blockbreakingdelayenabled,
    Horizontalcollision,
    Verticalcollision,
    Downleft,
    Downright,
    Startusingitem,
    Iscamerarelativemovementenabled,
    Isrotcontrolledbymovedirection,
    Startspinattack,
    Stopspinattack,
    Ishotbaronlytouch,
    Jumpreleasedraw,
    Jumppressedraw,
    Jumpcurrentraw,
    Sneakreleasedraw,
    Sneakpressedraw,
    Sneakcurrentraw,
    Internalupdate,
    Unknown(i32),
}

impl From<i32> for InputData {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Ascend,
            1 => Self::Descend,
            2 => Self::Northjump,
            3 => Self::Jumpdown,
            4 => Self::Sprintdown,
            5 => Self::Changeheight,
            6 => Self::Jumping,
            7 => Self::Autojumpinginwater,
            8 => Self::Sneaking,
            9 => Self::Sneakdown,
            10 => Self::Up,
            11 => Self::Down,
            12 => Self::Left,
            13 => Self::Right,
            14 => Self::Upleft,
            15 => Self::Upright,
            16 => Self::Wantup,
            17 => Self::Wantdown,
            18 => Self::Wantdownslow,
            19 => Self::Wantupslow,
            20 => Self::Sprinting,
            21 => Self::Ascendblock,
            22 => Self::Descendblock,
            23 => Self::Sneaktoggledown,
            24 => Self::Persistsneak,
            25 => Self::Startsprinting,
            26 => Self::Stopsprinting,
            27 => Self::Startsneaking,
            28 => Self::Stopsneaking,
            29 => Self::Startswimming,
            30 => Self::Stopswimming,
            31 => Self::Startjumping,
            32 => Self::Startgliding,
            33 => Self::Stopgliding,
            34 => Self::Performiteminteraction,
            35 => Self::Performblockactions,
            36 => Self::Performitemstackrequest,
            37 => Self::Handledteleport,
            38 => Self::Emoting,
            39 => Self::Missedswing,
            40 => Self::Startcrawling,
            41 => Self::Stopcrawling,
            42 => Self::Startflying,
            43 => Self::Stopflying,
            44 => Self::Clientackserverdata,
            45 => Self::Isinclientpredictedvehicle,
            46 => Self::Paddlingleft,
            47 => Self::Paddlingright,
            48 => Self::Blockbreakingdelayenabled,
            49 => Self::Horizontalcollision,
            50 => Self::Verticalcollision,
            51 => Self::Downleft,
            52 => Self::Downright,
            53 => Self::Startusingitem,
            54 => Self::Iscamerarelativemovementenabled,
            55 => Self::Isrotcontrolledbymovedirection,
            56 => Self::Startspinattack,
            57 => Self::Stopspinattack,
            58 => Self::Ishotbaronlytouch,
            59 => Self::Jumpreleasedraw,
            60 => Self::Jumppressedraw,
            61 => Self::Jumpcurrentraw,
            62 => Self::Sneakreleasedraw,
            63 => Self::Sneakpressedraw,
            64 => Self::Sneakcurrentraw,
            65 => Self::Internalupdate,
            value => Self::Unknown(value),
        }
    }
}

impl InputData {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Ascend => 0,
            Self::Descend => 1,
            Self::Northjump => 2,
            Self::Jumpdown => 3,
            Self::Sprintdown => 4,
            Self::Changeheight => 5,
            Self::Jumping => 6,
            Self::Autojumpinginwater => 7,
            Self::Sneaking => 8,
            Self::Sneakdown => 9,
            Self::Up => 10,
            Self::Down => 11,
            Self::Left => 12,
            Self::Right => 13,
            Self::Upleft => 14,
            Self::Upright => 15,
            Self::Wantup => 16,
            Self::Wantdown => 17,
            Self::Wantdownslow => 18,
            Self::Wantupslow => 19,
            Self::Sprinting => 20,
            Self::Ascendblock => 21,
            Self::Descendblock => 22,
            Self::Sneaktoggledown => 23,
            Self::Persistsneak => 24,
            Self::Startsprinting => 25,
            Self::Stopsprinting => 26,
            Self::Startsneaking => 27,
            Self::Stopsneaking => 28,
            Self::Startswimming => 29,
            Self::Stopswimming => 30,
            Self::Startjumping => 31,
            Self::Startgliding => 32,
            Self::Stopgliding => 33,
            Self::Performiteminteraction => 34,
            Self::Performblockactions => 35,
            Self::Performitemstackrequest => 36,
            Self::Handledteleport => 37,
            Self::Emoting => 38,
            Self::Missedswing => 39,
            Self::Startcrawling => 40,
            Self::Stopcrawling => 41,
            Self::Startflying => 42,
            Self::Stopflying => 43,
            Self::Clientackserverdata => 44,
            Self::Isinclientpredictedvehicle => 45,
            Self::Paddlingleft => 46,
            Self::Paddlingright => 47,
            Self::Blockbreakingdelayenabled => 48,
            Self::Horizontalcollision => 49,
            Self::Verticalcollision => 50,
            Self::Downleft => 51,
            Self::Downright => 52,
            Self::Startusingitem => 53,
            Self::Iscamerarelativemovementenabled => 54,
            Self::Isrotcontrolledbymovedirection => 55,
            Self::Startspinattack => 56,
            Self::Stopspinattack => 57,
            Self::Ishotbaronlytouch => 58,
            Self::Jumpreleasedraw => 59,
            Self::Jumppressedraw => 60,
            Self::Jumpcurrentraw => 61,
            Self::Sneakreleasedraw => 62,
            Self::Sneakpressedraw => 63,
            Self::Sneakcurrentraw => 64,
            Self::Internalupdate => 65,
            Self::Unknown(value) => value,
        }
    }
}

impl From<InputData> for i32 {
    fn from(value: InputData) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for InputData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for InputData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum InputMode {
    #[default]
    Undefined,
    Mouse,
    Touch,
    Gamepad,
    Motioncontroller,
    Count,
    Unknown(u32),
}

impl From<u32> for InputMode {
    fn from(value: u32) -> Self {
        match value {
            0 => Self::Undefined,
            1 => Self::Mouse,
            2 => Self::Touch,
            3 => Self::Gamepad,
            4 => Self::Motioncontroller,
            5 => Self::Count,
            value => Self::Unknown(value),
        }
    }
}

impl InputMode {
    pub fn to_raw(self) -> u32 {
        match self {
            Self::Undefined => 0,
            Self::Mouse => 1,
            Self::Touch => 2,
            Self::Gamepad => 3,
            Self::Motioncontroller => 4,
            Self::Count => 5,
            Self::Unknown(value) => value,
        }
    }
}

impl From<InputMode> for u32 {
    fn from(value: InputMode) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for InputMode {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::VarUInt(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for InputMode {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::VarUInt as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum InteractAction {
    #[default]
    Invalid,
    Stopriding,
    Interactupdate,
    Npcopen,
    Openinventory,
    Unknown(u8),
}

impl From<u8> for InteractAction {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Invalid,
            3 => Self::Stopriding,
            4 => Self::Interactupdate,
            5 => Self::Npcopen,
            6 => Self::Openinventory,
            value => Self::Unknown(value),
        }
    }
}

impl InteractAction {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Invalid => 0,
            Self::Stopriding => 3,
            Self::Interactupdate => 4,
            Self::Npcopen => 5,
            Self::Openinventory => 6,
            Self::Unknown(value) => value,
        }
    }
}

impl From<InteractAction> for u8 {
    fn from(value: InteractAction) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for InteractAction {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for InteractAction {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum LabTableReactionType {
    #[default]
    None,
    Icebomb,
    Bleach,
    Elephanttoothpaste,
    Fertilizer,
    Heatblock,
    Magnesiumsalts,
    Miscfire,
    Miscexplosion,
    Misclava,
    Miscmystical,
    Miscsmoke,
    Misclargesmoke,
    Unknown(u8),
}

impl From<u8> for LabTableReactionType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Icebomb,
            2 => Self::Bleach,
            3 => Self::Elephanttoothpaste,
            4 => Self::Fertilizer,
            5 => Self::Heatblock,
            6 => Self::Magnesiumsalts,
            7 => Self::Miscfire,
            8 => Self::Miscexplosion,
            9 => Self::Misclava,
            10 => Self::Miscmystical,
            11 => Self::Miscsmoke,
            12 => Self::Misclargesmoke,
            value => Self::Unknown(value),
        }
    }
}

impl LabTableReactionType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Icebomb => 1,
            Self::Bleach => 2,
            Self::Elephanttoothpaste => 3,
            Self::Fertilizer => 4,
            Self::Heatblock => 5,
            Self::Magnesiumsalts => 6,
            Self::Miscfire => 7,
            Self::Miscexplosion => 8,
            Self::Misclava => 9,
            Self::Miscmystical => 10,
            Self::Miscsmoke => 11,
            Self::Misclargesmoke => 12,
            Self::Unknown(value) => value,
        }
    }
}

impl From<LabTableReactionType> for u8 {
    fn from(value: LabTableReactionType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for LabTableReactionType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for LabTableReactionType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum LabTableType {
    #[default]
    Startcombine,
    Startreaction,
    Reset,
    Unknown(u8),
}

impl From<u8> for LabTableType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Startcombine,
            1 => Self::Startreaction,
            2 => Self::Reset,
            value => Self::Unknown(value),
        }
    }
}

impl LabTableType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Startcombine => 0,
            Self::Startreaction => 1,
            Self::Reset => 2,
            Self::Unknown(value) => value,
        }
    }
}

impl From<LabTableType> for u8 {
    fn from(value: LabTableType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for LabTableType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for LabTableType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum LegacyArmorSlot {
    #[default]
    Head,
    Torso,
    Legs,
    Feet,
    Body,
    Unknown(i32),
}

impl From<i32> for LegacyArmorSlot {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Head,
            1 => Self::Torso,
            2 => Self::Legs,
            3 => Self::Feet,
            4 => Self::Body,
            value => Self::Unknown(value),
        }
    }
}

impl LegacyArmorSlot {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Head => 0,
            Self::Torso => 1,
            Self::Legs => 2,
            Self::Feet => 3,
            Self::Body => 4,
            Self::Unknown(value) => value,
        }
    }
}

impl From<LegacyArmorSlot> for i32 {
    fn from(value: LegacyArmorSlot) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for LegacyArmorSlot {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for LegacyArmorSlot {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum LegacyDifficulty {
    #[default]
    Peaceful,
    Easy,
    Normal,
    Hard,
    Count,
    Unknown,
    Unknown2(i32),
}

impl From<i32> for LegacyDifficulty {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Peaceful,
            1 => Self::Easy,
            2 => Self::Normal,
            3 => Self::Hard,
            4 => Self::Count,
            5 => Self::Unknown,
            value => Self::Unknown2(value),
        }
    }
}

impl LegacyDifficulty {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Peaceful => 0,
            Self::Easy => 1,
            Self::Normal => 2,
            Self::Hard => 3,
            Self::Count => 4,
            Self::Unknown => 5,
            Self::Unknown2(value) => value,
        }
    }
}

impl From<LegacyDifficulty> for i32 {
    fn from(value: LegacyDifficulty) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for LegacyDifficulty {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for LegacyDifficulty {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum MinecraftEventingAchievementIds {
    #[default]
    Chestfullofcobblestone,
    Diamondforyou,
    Ironbelly,
    Ironman,
    Onarail,
    Overkill,
    Returntosender,
    Sniperduel,
    Stayinfrosty,
    Takeinventory,
    Maproom,
    Freightstation,
    Smelteverything,
    Tasteofyourownmedicine,
    Whenpigsfly,
    Inception,
    Artificialselection,
    Freediver,
    Spawnthewither,
    Beaconator,
    Greatview,
    Supersonic,
    Theendagain,
    Treasurehunter,
    Shootingstar,
    Fashionshow,
    Selfpublishedauthor,
    Alternativefuel,
    Sleepwiththefishes,
    Castaway,
    Imamarinebiologist,
    Sailthe7seas,
    Megold,
    Ahoy,
    Atlantis,
    Onepickletwopickleseapicklefour,
    Doabarrelroll,
    Moskstraumen,
    Echolocation,
    Wherehaveyoubeen,
    Topoftheworld,
    Fruitontheloom,
    Soundthealarm,
    Buylowsellhigh,
    Disenchanted,
    Timeforstew,
    Beeourguest,
    Totalbeelocation,
    Stickysituation,
    Covermeindebris,
    Floatyourgoat,
    Friend,
    Waxonwaxoff,
    Striderriddeninlavainoverworld,
    Goathornacquired,
    Jukeboxusedinmeadows,
    Tradedatworldheight,
    Survivedfallfromworldheight,
    Sneakclosetosculksensor,
    Itspreads,
    Birthdaysong,
    Withourpowerscombined,
    Plantingthepast,
    Carefulrestoration,
    Revaulting,
    Crafterscraftingcrafters,
    Whoneedsrockets,
    Overoverkill,
    Hearttransplanter,
    Stayhydrated,
    Mobkabob,
    Adventuringtime,
    Uhoh,
    Gettingwood,
    Benchmaking,
    Timetomine,
    Hottopic,
    Acquirehardware,
    Gettinganupgrade,
    Monsterhunter,
    Diamonds,
    Plethoraofcats,
    Unknown(u8),
}

impl From<u8> for MinecraftEventingAchievementIds {
    fn from(value: u8) -> Self {
        match value {
            7 => Self::Chestfullofcobblestone,
            10 => Self::Diamondforyou,
            20 => Self::Ironbelly,
            21 => Self::Ironman,
            29 => Self::Onarail,
            30 => Self::Overkill,
            37 => Self::Returntosender,
            38 => Self::Sniperduel,
            39 => Self::Stayinfrosty,
            40 => Self::Takeinventory,
            50 => Self::Maproom,
            52 => Self::Freightstation,
            53 => Self::Smelteverything,
            54 => Self::Tasteofyourownmedicine,
            56 => Self::Whenpigsfly,
            58 => Self::Inception,
            60 => Self::Artificialselection,
            61 => Self::Freediver,
            62 => Self::Spawnthewither,
            63 => Self::Beaconator,
            64 => Self::Greatview,
            65 => Self::Supersonic,
            66 => Self::Theendagain,
            67 => Self::Treasurehunter,
            68 => Self::Shootingstar,
            69 => Self::Fashionshow,
            71 => Self::Selfpublishedauthor,
            72 => Self::Alternativefuel,
            73 => Self::Sleepwiththefishes,
            74 => Self::Castaway,
            75 => Self::Imamarinebiologist,
            76 => Self::Sailthe7seas,
            77 => Self::Megold,
            78 => Self::Ahoy,
            79 => Self::Atlantis,
            80 => Self::Onepickletwopickleseapicklefour,
            81 => Self::Doabarrelroll,
            82 => Self::Moskstraumen,
            83 => Self::Echolocation,
            84 => Self::Wherehaveyoubeen,
            85 => Self::Topoftheworld,
            86 => Self::Fruitontheloom,
            87 => Self::Soundthealarm,
            88 => Self::Buylowsellhigh,
            89 => Self::Disenchanted,
            90 => Self::Timeforstew,
            91 => Self::Beeourguest,
            92 => Self::Totalbeelocation,
            93 => Self::Stickysituation,
            94 => Self::Covermeindebris,
            95 => Self::Floatyourgoat,
            96 => Self::Friend,
            97 => Self::Waxonwaxoff,
            98 => Self::Striderriddeninlavainoverworld,
            99 => Self::Goathornacquired,
            100 => Self::Jukeboxusedinmeadows,
            101 => Self::Tradedatworldheight,
            102 => Self::Survivedfallfromworldheight,
            103 => Self::Sneakclosetosculksensor,
            104 => Self::Itspreads,
            105 => Self::Birthdaysong,
            106 => Self::Withourpowerscombined,
            107 => Self::Plantingthepast,
            108 => Self::Carefulrestoration,
            109 => Self::Revaulting,
            110 => Self::Crafterscraftingcrafters,
            111 => Self::Whoneedsrockets,
            112 => Self::Overoverkill,
            113 => Self::Hearttransplanter,
            114 => Self::Stayhydrated,
            115 => Self::Mobkabob,
            116 => Self::Adventuringtime,
            117 => Self::Uhoh,
            118 => Self::Gettingwood,
            119 => Self::Benchmaking,
            120 => Self::Timetomine,
            121 => Self::Hottopic,
            122 => Self::Acquirehardware,
            123 => Self::Gettinganupgrade,
            124 => Self::Monsterhunter,
            125 => Self::Diamonds,
            126 => Self::Plethoraofcats,
            value => Self::Unknown(value),
        }
    }
}

impl MinecraftEventingAchievementIds {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Chestfullofcobblestone => 7,
            Self::Diamondforyou => 10,
            Self::Ironbelly => 20,
            Self::Ironman => 21,
            Self::Onarail => 29,
            Self::Overkill => 30,
            Self::Returntosender => 37,
            Self::Sniperduel => 38,
            Self::Stayinfrosty => 39,
            Self::Takeinventory => 40,
            Self::Maproom => 50,
            Self::Freightstation => 52,
            Self::Smelteverything => 53,
            Self::Tasteofyourownmedicine => 54,
            Self::Whenpigsfly => 56,
            Self::Inception => 58,
            Self::Artificialselection => 60,
            Self::Freediver => 61,
            Self::Spawnthewither => 62,
            Self::Beaconator => 63,
            Self::Greatview => 64,
            Self::Supersonic => 65,
            Self::Theendagain => 66,
            Self::Treasurehunter => 67,
            Self::Shootingstar => 68,
            Self::Fashionshow => 69,
            Self::Selfpublishedauthor => 71,
            Self::Alternativefuel => 72,
            Self::Sleepwiththefishes => 73,
            Self::Castaway => 74,
            Self::Imamarinebiologist => 75,
            Self::Sailthe7seas => 76,
            Self::Megold => 77,
            Self::Ahoy => 78,
            Self::Atlantis => 79,
            Self::Onepickletwopickleseapicklefour => 80,
            Self::Doabarrelroll => 81,
            Self::Moskstraumen => 82,
            Self::Echolocation => 83,
            Self::Wherehaveyoubeen => 84,
            Self::Topoftheworld => 85,
            Self::Fruitontheloom => 86,
            Self::Soundthealarm => 87,
            Self::Buylowsellhigh => 88,
            Self::Disenchanted => 89,
            Self::Timeforstew => 90,
            Self::Beeourguest => 91,
            Self::Totalbeelocation => 92,
            Self::Stickysituation => 93,
            Self::Covermeindebris => 94,
            Self::Floatyourgoat => 95,
            Self::Friend => 96,
            Self::Waxonwaxoff => 97,
            Self::Striderriddeninlavainoverworld => 98,
            Self::Goathornacquired => 99,
            Self::Jukeboxusedinmeadows => 100,
            Self::Tradedatworldheight => 101,
            Self::Survivedfallfromworldheight => 102,
            Self::Sneakclosetosculksensor => 103,
            Self::Itspreads => 104,
            Self::Birthdaysong => 105,
            Self::Withourpowerscombined => 106,
            Self::Plantingthepast => 107,
            Self::Carefulrestoration => 108,
            Self::Revaulting => 109,
            Self::Crafterscraftingcrafters => 110,
            Self::Whoneedsrockets => 111,
            Self::Overoverkill => 112,
            Self::Hearttransplanter => 113,
            Self::Stayhydrated => 114,
            Self::Mobkabob => 115,
            Self::Adventuringtime => 116,
            Self::Uhoh => 117,
            Self::Gettingwood => 118,
            Self::Benchmaking => 119,
            Self::Timetomine => 120,
            Self::Hottopic => 121,
            Self::Acquirehardware => 122,
            Self::Gettinganupgrade => 123,
            Self::Monsterhunter => 124,
            Self::Diamonds => 125,
            Self::Plethoraofcats => 126,
            Self::Unknown(value) => value,
        }
    }
}

impl From<MinecraftEventingAchievementIds> for u8 {
    fn from(value: MinecraftEventingAchievementIds) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for MinecraftEventingAchievementIds {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for MinecraftEventingAchievementIds {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum MinecraftEventingInteractionType {
    #[default]
    Breeding,
    Taming,
    Curing,
    Crafted,
    Shearing,
    Milking,
    Trading,
    Feeding,
    Igniting,
    Coloring,
    Naming,
    Leashing,
    Unleashing,
    Petsleep,
    Trusting,
    Commanding,
    Equipping,
    Unknown(u8),
}

impl From<u8> for MinecraftEventingInteractionType {
    fn from(value: u8) -> Self {
        match value {
            1 => Self::Breeding,
            2 => Self::Taming,
            3 => Self::Curing,
            4 => Self::Crafted,
            5 => Self::Shearing,
            6 => Self::Milking,
            7 => Self::Trading,
            8 => Self::Feeding,
            9 => Self::Igniting,
            10 => Self::Coloring,
            11 => Self::Naming,
            12 => Self::Leashing,
            13 => Self::Unleashing,
            14 => Self::Petsleep,
            15 => Self::Trusting,
            16 => Self::Commanding,
            17 => Self::Equipping,
            value => Self::Unknown(value),
        }
    }
}

impl MinecraftEventingInteractionType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Breeding => 1,
            Self::Taming => 2,
            Self::Curing => 3,
            Self::Crafted => 4,
            Self::Shearing => 5,
            Self::Milking => 6,
            Self::Trading => 7,
            Self::Feeding => 8,
            Self::Igniting => 9,
            Self::Coloring => 10,
            Self::Naming => 11,
            Self::Leashing => 12,
            Self::Unleashing => 13,
            Self::Petsleep => 14,
            Self::Trusting => 15,
            Self::Commanding => 16,
            Self::Equipping => 17,
            Self::Unknown(value) => value,
        }
    }
}

impl From<MinecraftEventingInteractionType> for u8 {
    fn from(value: MinecraftEventingInteractionType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for MinecraftEventingInteractionType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for MinecraftEventingInteractionType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum MinecraftEventingPOIBlockInteractionType {
    #[default]
    None,
    Extend,
    Clone,
    Lock,
    Create,
    Createlocator,
    Rename,
    Itemplaced,
    Itemremoved,
    Cooking,
    Dousing,
    Lighting,
    Haystack,
    Filled,
    Emptied,
    Adddye,
    Dyeitem,
    Clearitem,
    Enchantarrow,
    Compostitemplaced,
    Recoveredbonemeal,
    Bookplaced,
    Bookopened,
    Disenchant,
    Repair,
    Disenchantandrepair,
    Unknown(u8),
}

impl From<u8> for MinecraftEventingPOIBlockInteractionType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Extend,
            2 => Self::Clone,
            3 => Self::Lock,
            4 => Self::Create,
            5 => Self::Createlocator,
            6 => Self::Rename,
            7 => Self::Itemplaced,
            8 => Self::Itemremoved,
            9 => Self::Cooking,
            10 => Self::Dousing,
            11 => Self::Lighting,
            12 => Self::Haystack,
            13 => Self::Filled,
            14 => Self::Emptied,
            15 => Self::Adddye,
            16 => Self::Dyeitem,
            17 => Self::Clearitem,
            18 => Self::Enchantarrow,
            19 => Self::Compostitemplaced,
            20 => Self::Recoveredbonemeal,
            21 => Self::Bookplaced,
            22 => Self::Bookopened,
            23 => Self::Disenchant,
            24 => Self::Repair,
            25 => Self::Disenchantandrepair,
            value => Self::Unknown(value),
        }
    }
}

impl MinecraftEventingPOIBlockInteractionType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Extend => 1,
            Self::Clone => 2,
            Self::Lock => 3,
            Self::Create => 4,
            Self::Createlocator => 5,
            Self::Rename => 6,
            Self::Itemplaced => 7,
            Self::Itemremoved => 8,
            Self::Cooking => 9,
            Self::Dousing => 10,
            Self::Lighting => 11,
            Self::Haystack => 12,
            Self::Filled => 13,
            Self::Emptied => 14,
            Self::Adddye => 15,
            Self::Dyeitem => 16,
            Self::Clearitem => 17,
            Self::Enchantarrow => 18,
            Self::Compostitemplaced => 19,
            Self::Recoveredbonemeal => 20,
            Self::Bookplaced => 21,
            Self::Bookopened => 22,
            Self::Disenchant => 23,
            Self::Repair => 24,
            Self::Disenchantandrepair => 25,
            Self::Unknown(value) => value,
        }
    }
}

impl From<MinecraftEventingPOIBlockInteractionType> for u8 {
    fn from(value: MinecraftEventingPOIBlockInteractionType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for MinecraftEventingPOIBlockInteractionType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for MinecraftEventingPOIBlockInteractionType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Mirror {
    #[default]
    None,
    X,
    Z,
    Xz,
    Unknown(u8),
}

impl From<u8> for Mirror {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::None,
            1 => Self::X,
            2 => Self::Z,
            3 => Self::Xz,
            value => Self::Unknown(value),
        }
    }
}

impl Mirror {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::None => 0,
            Self::X => 1,
            Self::Z => 2,
            Self::Xz => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<Mirror> for u8 {
    fn from(value: Mirror) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for Mirror {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for Mirror {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum MoLangVersion {
    #[default]
    Invalid,
    Beforeversioning,
    Initial,
    Fixeditemremainingusedurationquery,
    Expressionerrormessages,
    Unexpectedoperatorerrors,
    Conditionaloperatorassociativity,
    Comparisonandlogicaloperatorprecedence,
    Dividebynegativevalue,
    Fixedcapeflapamountquery,
    Queryblockpropertyrenamedtostate,
    Deprecateoldblockquerynames,
    Deprecatedsnifferandcamelqueries,
    Leafsupportinginfirstsolidblockbelow,
    Latest,
    Numvalidversions,
    Unknown(i16),
}

impl From<i16> for MoLangVersion {
    fn from(value: i16) -> Self {
        match value {
            -1 => Self::Invalid,
            0 => Self::Beforeversioning,
            1 => Self::Initial,
            2 => Self::Fixeditemremainingusedurationquery,
            3 => Self::Expressionerrormessages,
            4 => Self::Unexpectedoperatorerrors,
            5 => Self::Conditionaloperatorassociativity,
            6 => Self::Comparisonandlogicaloperatorprecedence,
            7 => Self::Dividebynegativevalue,
            8 => Self::Fixedcapeflapamountquery,
            9 => Self::Queryblockpropertyrenamedtostate,
            10 => Self::Deprecateoldblockquerynames,
            11 => Self::Deprecatedsnifferandcamelqueries,
            12 => Self::Leafsupportinginfirstsolidblockbelow,
            13 => Self::Latest,
            14 => Self::Numvalidversions,
            value => Self::Unknown(value),
        }
    }
}

impl MoLangVersion {
    pub fn to_raw(self) -> i16 {
        match self {
            Self::Invalid => -1,
            Self::Beforeversioning => 0,
            Self::Initial => 1,
            Self::Fixeditemremainingusedurationquery => 2,
            Self::Expressionerrormessages => 3,
            Self::Unexpectedoperatorerrors => 4,
            Self::Conditionaloperatorassociativity => 5,
            Self::Comparisonandlogicaloperatorprecedence => 6,
            Self::Dividebynegativevalue => 7,
            Self::Fixedcapeflapamountquery => 8,
            Self::Queryblockpropertyrenamedtostate => 9,
            Self::Deprecateoldblockquerynames => 10,
            Self::Deprecatedsnifferandcamelqueries => 11,
            Self::Leafsupportinginfirstsolidblockbelow => 12,
            Self::Latest => 13,
            Self::Numvalidversions => 14,
            Self::Unknown(value) => value,
        }
    }
}

impl From<MoLangVersion> for i16 {
    fn from(value: MoLangVersion) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for MoLangVersion {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::I16LE(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for MoLangVersion {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::I16LE as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum MobEffectEvent {
    #[default]
    Invalid,
    Add,
    Update,
    Remove,
    Unknown(u8),
}

impl From<u8> for MobEffectEvent {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Invalid,
            1 => Self::Add,
            2 => Self::Update,
            3 => Self::Remove,
            value => Self::Unknown(value),
        }
    }
}

impl MobEffectEvent {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Invalid => 0,
            Self::Add => 1,
            Self::Update => 2,
            Self::Remove => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<MobEffectEvent> for u8 {
    fn from(value: MobEffectEvent) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for MobEffectEvent {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for MobEffectEvent {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ModalFormCancelReason {
    #[default]
    Userclosed,
    Userbusy,
    Unknown(u8),
}

impl From<u8> for ModalFormCancelReason {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Userclosed,
            1 => Self::Userbusy,
            value => Self::Unknown(value),
        }
    }
}

impl ModalFormCancelReason {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Userclosed => 0,
            Self::Userbusy => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ModalFormCancelReason> for u8 {
    fn from(value: ModalFormCancelReason) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ModalFormCancelReason {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ModalFormCancelReason {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum MovementEffectType {
    #[default]
    GlideBoost,
    DolphinBoost,
    GeyserBoost,
    Unknown(i32),
}

impl From<i32> for MovementEffectType {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::GlideBoost,
            1 => Self::DolphinBoost,
            2 => Self::GeyserBoost,
            value => Self::Unknown(value),
        }
    }
}

impl MovementEffectType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::GlideBoost => 0,
            Self::DolphinBoost => 1,
            Self::GeyserBoost => 2,
            Self::Unknown(value) => value,
        }
    }
}

impl From<MovementEffectType> for i32 {
    fn from(value: MovementEffectType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for MovementEffectType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for MovementEffectType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum MultiplayerSettingsType {
    #[default]
    Enablemultiplayer,
    Disablemultiplayer,
    Refreshjoincode,
    Unknown(i32),
}

impl From<i32> for MultiplayerSettingsType {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Enablemultiplayer,
            1 => Self::Disablemultiplayer,
            2 => Self::Refreshjoincode,
            value => Self::Unknown(value),
        }
    }
}

impl MultiplayerSettingsType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Enablemultiplayer => 0,
            Self::Disablemultiplayer => 1,
            Self::Refreshjoincode => 2,
            Self::Unknown(value) => value,
        }
    }
}

impl From<MultiplayerSettingsType> for i32 {
    fn from(value: MultiplayerSettingsType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for MultiplayerSettingsType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for MultiplayerSettingsType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum NewInteractionModel {
    #[default]
    Touch,
    Crosshair,
    Classic,
    Count,
    Unknown(i32),
}

impl From<i32> for NewInteractionModel {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Touch,
            1 => Self::Crosshair,
            2 => Self::Classic,
            3 => Self::Count,
            value => Self::Unknown(value),
        }
    }
}

impl NewInteractionModel {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Touch => 0,
            Self::Crosshair => 1,
            Self::Classic => 2,
            Self::Count => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<NewInteractionModel> for i32 {
    fn from(value: NewInteractionModel) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for NewInteractionModel {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for NewInteractionModel {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PacketCompressionAlgorithm {
    #[default]
    Zlib,
    Snappy,
    None,
    Unknown(u16),
}

impl From<u16> for PacketCompressionAlgorithm {
    fn from(value: u16) -> Self {
        match value {
            0 => Self::Zlib,
            1 => Self::Snappy,
            65535 => Self::None,
            value => Self::Unknown(value),
        }
    }
}

impl PacketCompressionAlgorithm {
    pub fn to_raw(self) -> u16 {
        match self {
            Self::Zlib => 0,
            Self::Snappy => 1,
            Self::None => 65535,
            Self::Unknown(value) => value,
        }
    }
}

impl From<PacketCompressionAlgorithm> for u16 {
    fn from(value: PacketCompressionAlgorithm) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for PacketCompressionAlgorithm {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U16LE(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PacketCompressionAlgorithm {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U16LE as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PacketType {
    #[default]
    Empty,
    Initiallyunlockedrecipes,
    Newlyunlockedrecipes,
    Removeunlockedrecipes,
    Removeallunlockedrecipes,
    Unknown(u32),
}

impl From<u32> for PacketType {
    fn from(value: u32) -> Self {
        match value {
            0 => Self::Empty,
            1 => Self::Initiallyunlockedrecipes,
            2 => Self::Newlyunlockedrecipes,
            3 => Self::Removeunlockedrecipes,
            4 => Self::Removeallunlockedrecipes,
            value => Self::Unknown(value),
        }
    }
}

impl PacketType {
    pub fn to_raw(self) -> u32 {
        match self {
            Self::Empty => 0,
            Self::Initiallyunlockedrecipes => 1,
            Self::Newlyunlockedrecipes => 2,
            Self::Removeunlockedrecipes => 3,
            Self::Removeallunlockedrecipes => 4,
            Self::Unknown(value) => value,
        }
    }
}

impl From<PacketType> for u32 {
    fn from(value: PacketType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for PacketType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U32LE(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PacketType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U32LE as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PacketViolationSeverity {
    #[default]
    Unknown,
    Warning,
    Finalwarning,
    Terminatingconnection,
    Unknown2(i32),
}

impl From<i32> for PacketViolationSeverity {
    fn from(value: i32) -> Self {
        match value {
            -1 => Self::Unknown,
            0 => Self::Warning,
            1 => Self::Finalwarning,
            2 => Self::Terminatingconnection,
            value => Self::Unknown2(value),
        }
    }
}

impl PacketViolationSeverity {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Unknown => -1,
            Self::Warning => 0,
            Self::Finalwarning => 1,
            Self::Terminatingconnection => 2,
            Self::Unknown2(value) => value,
        }
    }
}

impl From<PacketViolationSeverity> for i32 {
    fn from(value: PacketViolationSeverity) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for PacketViolationSeverity {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PacketViolationSeverity {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PacketViolationType {
    #[default]
    Unknown,
    Packetmalformed,
    Unknown2(i32),
}

impl From<i32> for PacketViolationType {
    fn from(value: i32) -> Self {
        match value {
            -1 => Self::Unknown,
            0 => Self::Packetmalformed,
            value => Self::Unknown2(value),
        }
    }
}

impl PacketViolationType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Unknown => -1,
            Self::Packetmalformed => 0,
            Self::Unknown2(value) => value,
        }
    }
}

impl From<PacketViolationType> for i32 {
    fn from(value: PacketViolationType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for PacketViolationType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PacketViolationType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PersonaAnimatedTextureType {
    #[default]
    None,
    Face,
    Body32x32,
    Body128x128,
    Unknown(u32),
}

impl From<u32> for PersonaAnimatedTextureType {
    fn from(value: u32) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Face,
            2 => Self::Body32x32,
            3 => Self::Body128x128,
            value => Self::Unknown(value),
        }
    }
}

impl PersonaAnimatedTextureType {
    pub fn to_raw(self) -> u32 {
        match self {
            Self::None => 0,
            Self::Face => 1,
            Self::Body32x32 => 2,
            Self::Body128x128 => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<PersonaAnimatedTextureType> for u32 {
    fn from(value: PersonaAnimatedTextureType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for PersonaAnimatedTextureType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::VarUInt(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PersonaAnimatedTextureType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::VarUInt as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PersonaAnimationExpression {
    #[default]
    Linear,
    Blinking,
    Unknown(u32),
}

impl From<u32> for PersonaAnimationExpression {
    fn from(value: u32) -> Self {
        match value {
            0 => Self::Linear,
            1 => Self::Blinking,
            value => Self::Unknown(value),
        }
    }
}

impl PersonaAnimationExpression {
    pub fn to_raw(self) -> u32 {
        match self {
            Self::Linear => 0,
            Self::Blinking => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<PersonaAnimationExpression> for u32 {
    fn from(value: PersonaAnimationExpression) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for PersonaAnimationExpression {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::VarUInt(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PersonaAnimationExpression {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::VarUInt as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PersonaArmSizeType {
    #[default]
    Slim,
    Wide,
    Unknown(u8),
}

impl From<u8> for PersonaArmSizeType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Slim,
            1 => Self::Wide,
            value => Self::Unknown(value),
        }
    }
}

impl PersonaArmSizeType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Slim => 0,
            Self::Wide => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<PersonaArmSizeType> for u8 {
    fn from(value: PersonaArmSizeType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for PersonaArmSizeType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PersonaArmSizeType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PhotoType {
    #[default]
    Portfolio,
    Photoitem,
    Book,
    Unknown(u8),
}

impl From<u8> for PhotoType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Portfolio,
            1 => Self::Photoitem,
            2 => Self::Book,
            value => Self::Unknown(value),
        }
    }
}

impl PhotoType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Portfolio => 0,
            Self::Photoitem => 1,
            Self::Book => 2,
            Self::Unknown(value) => value,
        }
    }
}

impl From<PhotoType> for u8 {
    fn from(value: PhotoType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for PhotoType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PhotoType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PlayStatusType {
    #[default]
    Loginsuccess,
    LoginfailedClientold,
    LoginfailedServerold,
    Playerspawn,
    LoginfailedInvalidtenant,
    LoginfailedEditionmismatchedutovanilla,
    LoginfailedEditionmismatchvanillatoedu,
    LoginfailedServerfullsubclient,
    LoginfailedEditormismatcheditortovanilla,
    LoginfailedEditormismatchvanillatoeditor,
    Unknown(i32),
}

impl From<i32> for PlayStatusType {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Loginsuccess,
            1 => Self::LoginfailedClientold,
            2 => Self::LoginfailedServerold,
            3 => Self::Playerspawn,
            4 => Self::LoginfailedInvalidtenant,
            5 => Self::LoginfailedEditionmismatchedutovanilla,
            6 => Self::LoginfailedEditionmismatchvanillatoedu,
            7 => Self::LoginfailedServerfullsubclient,
            8 => Self::LoginfailedEditormismatcheditortovanilla,
            9 => Self::LoginfailedEditormismatchvanillatoeditor,
            value => Self::Unknown(value),
        }
    }
}

impl PlayStatusType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Loginsuccess => 0,
            Self::LoginfailedClientold => 1,
            Self::LoginfailedServerold => 2,
            Self::Playerspawn => 3,
            Self::LoginfailedInvalidtenant => 4,
            Self::LoginfailedEditionmismatchedutovanilla => 5,
            Self::LoginfailedEditionmismatchvanillatoedu => 6,
            Self::LoginfailedServerfullsubclient => 7,
            Self::LoginfailedEditormismatcheditortovanilla => 8,
            Self::LoginfailedEditormismatchvanillatoeditor => 9,
            Self::Unknown(value) => value,
        }
    }
}

impl From<PlayStatusType> for i32 {
    fn from(value: PlayStatusType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for PlayStatusType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::I32BE(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PlayStatusType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::I32BE as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum RandomDistributionType {
    #[default]
    Singlevalued,
    Uniform,
    Gaussian,
    Inversegaussian,
    Fixedgrid,
    Jitteredgrid,
    Triangle,
    Unknown(i32),
}

impl From<i32> for RandomDistributionType {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Singlevalued,
            1 => Self::Uniform,
            2 => Self::Gaussian,
            3 => Self::Inversegaussian,
            4 => Self::Fixedgrid,
            5 => Self::Jitteredgrid,
            6 => Self::Triangle,
            value => Self::Unknown(value),
        }
    }
}

impl RandomDistributionType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Singlevalued => 0,
            Self::Uniform => 1,
            Self::Gaussian => 2,
            Self::Inversegaussian => 3,
            Self::Fixedgrid => 4,
            Self::Jitteredgrid => 5,
            Self::Triangle => 6,
            Self::Unknown(value) => value,
        }
    }
}

impl From<RandomDistributionType> for i32 {
    fn from(value: RandomDistributionType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for RandomDistributionType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for RandomDistributionType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum RequestAbilityType {
    #[default]
    Unset,
    Bool,
    Float,
    Unknown(u8),
}

impl From<u8> for RequestAbilityType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Unset,
            1 => Self::Bool,
            2 => Self::Float,
            value => Self::Unknown(value),
        }
    }
}

impl RequestAbilityType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Unset => 0,
            Self::Bool => 1,
            Self::Float => 2,
            Self::Unknown(value) => value,
        }
    }
}

impl From<RequestAbilityType> for u8 {
    fn from(value: RequestAbilityType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for RequestAbilityType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for RequestAbilityType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum RequestType {
    #[default]
    Setactions,
    Executeaction,
    Executeclosingcommands,
    Setname,
    Setskin,
    Setinteracttext,
    Executeopeningcommands,
    Unknown(u8),
}

impl From<u8> for RequestType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Setactions,
            1 => Self::Executeaction,
            2 => Self::Executeclosingcommands,
            3 => Self::Setname,
            4 => Self::Setskin,
            5 => Self::Setinteracttext,
            6 => Self::Executeopeningcommands,
            value => Self::Unknown(value),
        }
    }
}

impl RequestType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Setactions => 0,
            Self::Executeaction => 1,
            Self::Executeclosingcommands => 2,
            Self::Setname => 3,
            Self::Setskin => 4,
            Self::Setinteracttext => 5,
            Self::Executeopeningcommands => 6,
            Self::Unknown(value) => value,
        }
    }
}

impl From<RequestType> for u8 {
    fn from(value: RequestType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for RequestType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for RequestType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum RewindType {
    #[default]
    Player,
    Vehicle,
    Unknown(u8),
}

impl From<u8> for RewindType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Player,
            1 => Self::Vehicle,
            value => Self::Unknown(value),
        }
    }
}

impl RewindType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Player => 0,
            Self::Vehicle => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<RewindType> for u8 {
    fn from(value: RewindType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for RewindType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for RewindType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Rotation {
    #[default]
    None,
    Rotate90,
    Rotate180,
    Rotate270,
    Unknown(u8),
}

impl From<u8> for Rotation {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Rotate90,
            2 => Self::Rotate180,
            3 => Self::Rotate270,
            value => Self::Unknown(value),
        }
    }
}

impl Rotation {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Rotate90 => 1,
            Self::Rotate180 => 2,
            Self::Rotate270 => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<Rotation> for u8 {
    fn from(value: Rotation) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for Rotation {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for Rotation {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ScriptModuleMinecraftScriptPrimitiveShapeType {
    #[default]
    Line,
    Box,
    Sphere,
    Circle,
    Text,
    Arrow,
    Cylinder,
    Pyramid,
    Ellipsoid,
    Cone,
    Unknown(u8),
}

impl From<u8> for ScriptModuleMinecraftScriptPrimitiveShapeType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Line,
            1 => Self::Box,
            2 => Self::Sphere,
            3 => Self::Circle,
            4 => Self::Text,
            5 => Self::Arrow,
            6 => Self::Cylinder,
            7 => Self::Pyramid,
            8 => Self::Ellipsoid,
            9 => Self::Cone,
            value => Self::Unknown(value),
        }
    }
}

impl ScriptModuleMinecraftScriptPrimitiveShapeType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Line => 0,
            Self::Box => 1,
            Self::Sphere => 2,
            Self::Circle => 3,
            Self::Text => 4,
            Self::Arrow => 5,
            Self::Cylinder => 6,
            Self::Pyramid => 7,
            Self::Ellipsoid => 8,
            Self::Cone => 9,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ScriptModuleMinecraftScriptPrimitiveShapeType> for u8 {
    fn from(value: ScriptModuleMinecraftScriptPrimitiveShapeType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ScriptModuleMinecraftScriptPrimitiveShapeType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ScriptModuleMinecraftScriptPrimitiveShapeType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ServerEditorConnectionPolicy {
    #[default]
    Matchworldtype,
    Editoronly,
    Vanillaonly,
    Mixed,
    Unknown(i32),
}

impl From<i32> for ServerEditorConnectionPolicy {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Matchworldtype,
            1 => Self::Editoronly,
            2 => Self::Vanillaonly,
            3 => Self::Mixed,
            value => Self::Unknown(value),
        }
    }
}

impl ServerEditorConnectionPolicy {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Matchworldtype => 0,
            Self::Editoronly => 1,
            Self::Vanillaonly => 2,
            Self::Mixed => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ServerEditorConnectionPolicy> for i32 {
    fn from(value: ServerEditorConnectionPolicy) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ServerEditorConnectionPolicy {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ServerEditorConnectionPolicy {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ServerWaypointGroupAction {
    #[default]
    None,
    Add,
    Remove,
    Update,
    Unknown(u8),
}

impl From<u8> for ServerWaypointGroupAction {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Add,
            2 => Self::Remove,
            3 => Self::Update,
            value => Self::Unknown(value),
        }
    }
}

impl ServerWaypointGroupAction {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Add => 1,
            Self::Remove => 2,
            Self::Update => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ServerWaypointGroupAction> for u8 {
    fn from(value: ServerWaypointGroupAction) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ServerWaypointGroupAction {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ServerWaypointGroupAction {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ServerboundLoadingScreenType {
    #[default]
    Startloadingscreen,
    Endloadingscreen,
    Unknown(i32),
}

impl From<i32> for ServerboundLoadingScreenType {
    fn from(value: i32) -> Self {
        match value {
            1 => Self::Startloadingscreen,
            2 => Self::Endloadingscreen,
            value => Self::Unknown(value),
        }
    }
}

impl ServerboundLoadingScreenType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Startloadingscreen => 1,
            Self::Endloadingscreen => 2,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ServerboundLoadingScreenType> for i32 {
    fn from(value: ServerboundLoadingScreenType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ServerboundLoadingScreenType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ServerboundLoadingScreenType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ShowStoreOfferRedirectType {
    #[default]
    Marketplaceoffer,
    Dressingroomoffer,
    Thirdpartyserverpage,
    Unknown(u8),
}

impl From<u8> for ShowStoreOfferRedirectType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Marketplaceoffer,
            1 => Self::Dressingroomoffer,
            2 => Self::Thirdpartyserverpage,
            value => Self::Unknown(value),
        }
    }
}

impl ShowStoreOfferRedirectType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Marketplaceoffer => 0,
            Self::Dressingroomoffer => 1,
            Self::Thirdpartyserverpage => 2,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ShowStoreOfferRedirectType> for u8 {
    fn from(value: ShowStoreOfferRedirectType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ShowStoreOfferRedirectType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ShowStoreOfferRedirectType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum SimulationTypeEnum {
    #[default]
    Game,
    Editor,
    Test,
    Invalid,
    Unknown(u8),
}

impl From<u8> for SimulationTypeEnum {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Game,
            1 => Self::Editor,
            2 => Self::Test,
            3 => Self::Invalid,
            value => Self::Unknown(value),
        }
    }
}

impl SimulationTypeEnum {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Game => 0,
            Self::Editor => 1,
            Self::Test => 2,
            Self::Invalid => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<SimulationTypeEnum> for u8 {
    fn from(value: SimulationTypeEnum) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for SimulationTypeEnum {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SimulationTypeEnum {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum SocialGamePublishSetting {
    #[default]
    Nomultiplay,
    Inviteonly,
    Friendsonly,
    Friendsoffriends,
    Public,
    Unknown(i32),
}

impl From<i32> for SocialGamePublishSetting {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Nomultiplay,
            1 => Self::Inviteonly,
            2 => Self::Friendsonly,
            3 => Self::Friendsoffriends,
            4 => Self::Public,
            value => Self::Unknown(value),
        }
    }
}

impl SocialGamePublishSetting {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Nomultiplay => 0,
            Self::Inviteonly => 1,
            Self::Friendsonly => 2,
            Self::Friendsoffriends => 3,
            Self::Public => 4,
            Self::Unknown(value) => value,
        }
    }
}

impl From<SocialGamePublishSetting> for i32 {
    fn from(value: SocialGamePublishSetting) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for SocialGamePublishSetting {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SocialGamePublishSetting {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum SoftEnumUpdateType {
    #[default]
    Add,
    Remove,
    Replace,
    Unknown(u8),
}

impl From<u8> for SoftEnumUpdateType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Add,
            1 => Self::Remove,
            2 => Self::Replace,
            value => Self::Unknown(value),
        }
    }
}

impl SoftEnumUpdateType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Add => 0,
            Self::Remove => 1,
            Self::Replace => 2,
            Self::Unknown(value) => value,
        }
    }
}

impl From<SoftEnumUpdateType> for u8 {
    fn from(value: SoftEnumUpdateType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for SoftEnumUpdateType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SoftEnumUpdateType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum SpawnBiomeType {
    #[default]
    Default,
    Userdefined,
    Unknown(i16),
}

impl From<i16> for SpawnBiomeType {
    fn from(value: i16) -> Self {
        match value {
            0 => Self::Default,
            1 => Self::Userdefined,
            value => Self::Unknown(value),
        }
    }
}

impl SpawnBiomeType {
    pub fn to_raw(self) -> i16 {
        match self {
            Self::Default => 0,
            Self::Userdefined => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<SpawnBiomeType> for i16 {
    fn from(value: SpawnBiomeType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for SpawnBiomeType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::I16LE(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SpawnBiomeType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::I16LE as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum SpawnPositionType {
    #[default]
    Playerrespawn,
    Worldspawn,
    Unknown(i32),
}

impl From<i32> for SpawnPositionType {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Playerrespawn,
            1 => Self::Worldspawn,
            value => Self::Unknown(value),
        }
    }
}

impl SpawnPositionType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Playerrespawn => 0,
            Self::Worldspawn => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<SpawnPositionType> for i32 {
    fn from(value: SpawnPositionType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for SpawnPositionType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SpawnPositionType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Subtype {
    #[default]
    Uninitializedsubtype,
    Enablecommands,
    Disablecommands,
    Unlockworldtemplatesettings,
    Unknown(u16),
}

impl From<u16> for Subtype {
    fn from(value: u16) -> Self {
        match value {
            0 => Self::Uninitializedsubtype,
            1 => Self::Enablecommands,
            2 => Self::Disablecommands,
            3 => Self::Unlockworldtemplatesettings,
            value => Self::Unknown(value),
        }
    }
}

impl Subtype {
    pub fn to_raw(self) -> u16 {
        match self {
            Self::Uninitializedsubtype => 0,
            Self::Enablecommands => 1,
            Self::Disablecommands => 2,
            Self::Unlockworldtemplatesettings => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<Subtype> for u16 {
    fn from(value: Subtype) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for Subtype {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U16LE(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for Subtype {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U16LE as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum TargetMode {
    #[default]
    Angle,
    Distance,
    Unknown(u8),
}

impl From<u8> for TargetMode {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Angle,
            1 => Self::Distance,
            value => Self::Unknown(value),
        }
    }
}

impl TargetMode {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Angle => 0,
            Self::Distance => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<TargetMode> for u8 {
    fn from(value: TargetMode) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for TargetMode {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for TargetMode {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum TitleType {
    #[default]
    Clear,
    Reset,
    Title,
    Subtitle,
    Actionbar,
    Times,
    Titletextobject,
    Subtitletextobject,
    Actionbartextobject,
    Unknown(i32),
}

impl From<i32> for TitleType {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Clear,
            1 => Self::Reset,
            2 => Self::Title,
            3 => Self::Subtitle,
            4 => Self::Actionbar,
            5 => Self::Times,
            6 => Self::Titletextobject,
            7 => Self::Subtitletextobject,
            8 => Self::Actionbartextobject,
            value => Self::Unknown(value),
        }
    }
}

impl TitleType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Clear => 0,
            Self::Reset => 1,
            Self::Title => 2,
            Self::Subtitle => 3,
            Self::Actionbar => 4,
            Self::Times => 5,
            Self::Titletextobject => 6,
            Self::Subtitletextobject => 7,
            Self::Actionbartextobject => 8,
            Self::Unknown(value) => value,
        }
    }
}

impl From<TitleType> for i32 {
    fn from(value: TitleType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for TitleType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for TitleType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum VillageType {
    #[default]
    Desert,
    Ice,
    Savanna,
    Taiga,
    Default,
    Unknown(u8),
}

impl From<u8> for VillageType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Desert,
            1 => Self::Ice,
            2 => Self::Savanna,
            3 => Self::Taiga,
            4 => Self::Default,
            value => Self::Unknown(value),
        }
    }
}

impl VillageType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Desert => 0,
            Self::Ice => 1,
            Self::Savanna => 2,
            Self::Taiga => 3,
            Self::Default => 4,
            Self::Unknown(value) => value,
        }
    }
}

impl From<VillageType> for u8 {
    fn from(value: VillageType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for VillageType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for VillageType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

// Domain: npc

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum NpcDialogueActionType {
    #[default]
    Open,
    Close,
    Unknown(i32),
}

impl From<i32> for NpcDialogueActionType {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Open,
            1 => Self::Close,
            value => Self::Unknown(value),
        }
    }
}

impl NpcDialogueActionType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Open => 0,
            Self::Close => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<NpcDialogueActionType> for i32 {
    fn from(value: NpcDialogueActionType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for NpcDialogueActionType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for NpcDialogueActionType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

// Domain: player

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PlayerActionType {
    #[default]
    Unknown,
    Startdestroyblock,
    Abortdestroyblock,
    Stopdestroyblock,
    Getupdatedblock,
    Dropitem,
    Startsleeping,
    Stopsleeping,
    Respawn,
    Startjump,
    Startsprinting,
    Stopsprinting,
    Startsneaking,
    Stopsneaking,
    Creativedestroyblock,
    Changedimensionack,
    Startgliding,
    Stopgliding,
    Denydestroyblock,
    Crackblock,
    Changeskin,
    Updatedenchantingseed,
    Startswimming,
    Stopswimming,
    Startspinattack,
    Stopspinattack,
    Interactwithblock,
    Predictdestroyblock,
    Continuedestroyblock,
    Startitemuseon,
    Stopitemuseon,
    Handledteleport,
    Missedswing,
    Startcrawling,
    Stopcrawling,
    Startflying,
    Stopflying,
    Clientackserverdata,
    Startusingitem,
    Internalupdate,
    Count,
    Unknown2(i32),
}

impl From<i32> for PlayerActionType {
    fn from(value: i32) -> Self {
        match value {
            -1 => Self::Unknown,
            0 => Self::Startdestroyblock,
            1 => Self::Abortdestroyblock,
            2 => Self::Stopdestroyblock,
            3 => Self::Getupdatedblock,
            4 => Self::Dropitem,
            5 => Self::Startsleeping,
            6 => Self::Stopsleeping,
            7 => Self::Respawn,
            8 => Self::Startjump,
            9 => Self::Startsprinting,
            10 => Self::Stopsprinting,
            11 => Self::Startsneaking,
            12 => Self::Stopsneaking,
            13 => Self::Creativedestroyblock,
            14 => Self::Changedimensionack,
            15 => Self::Startgliding,
            16 => Self::Stopgliding,
            17 => Self::Denydestroyblock,
            18 => Self::Crackblock,
            19 => Self::Changeskin,
            20 => Self::Updatedenchantingseed,
            21 => Self::Startswimming,
            22 => Self::Stopswimming,
            23 => Self::Startspinattack,
            24 => Self::Stopspinattack,
            25 => Self::Interactwithblock,
            26 => Self::Predictdestroyblock,
            27 => Self::Continuedestroyblock,
            28 => Self::Startitemuseon,
            29 => Self::Stopitemuseon,
            30 => Self::Handledteleport,
            31 => Self::Missedswing,
            32 => Self::Startcrawling,
            33 => Self::Stopcrawling,
            34 => Self::Startflying,
            35 => Self::Stopflying,
            36 => Self::Clientackserverdata,
            37 => Self::Startusingitem,
            38 => Self::Internalupdate,
            39 => Self::Count,
            value => Self::Unknown2(value),
        }
    }
}

impl PlayerActionType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Unknown => -1,
            Self::Startdestroyblock => 0,
            Self::Abortdestroyblock => 1,
            Self::Stopdestroyblock => 2,
            Self::Getupdatedblock => 3,
            Self::Dropitem => 4,
            Self::Startsleeping => 5,
            Self::Stopsleeping => 6,
            Self::Respawn => 7,
            Self::Startjump => 8,
            Self::Startsprinting => 9,
            Self::Stopsprinting => 10,
            Self::Startsneaking => 11,
            Self::Stopsneaking => 12,
            Self::Creativedestroyblock => 13,
            Self::Changedimensionack => 14,
            Self::Startgliding => 15,
            Self::Stopgliding => 16,
            Self::Denydestroyblock => 17,
            Self::Crackblock => 18,
            Self::Changeskin => 19,
            Self::Updatedenchantingseed => 20,
            Self::Startswimming => 21,
            Self::Stopswimming => 22,
            Self::Startspinattack => 23,
            Self::Stopspinattack => 24,
            Self::Interactwithblock => 25,
            Self::Predictdestroyblock => 26,
            Self::Continuedestroyblock => 27,
            Self::Startitemuseon => 28,
            Self::Stopitemuseon => 29,
            Self::Handledteleport => 30,
            Self::Missedswing => 31,
            Self::Startcrawling => 32,
            Self::Stopcrawling => 33,
            Self::Startflying => 34,
            Self::Stopflying => 35,
            Self::Clientackserverdata => 36,
            Self::Startusingitem => 37,
            Self::Internalupdate => 38,
            Self::Count => 39,
            Self::Unknown2(value) => value,
        }
    }
}

impl From<PlayerActionType> for i32 {
    fn from(value: PlayerActionType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for PlayerActionType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PlayerActionType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PlayerListPacketType {
    #[default]
    Add,
    Remove,
    Unknown(u8),
}

impl From<u8> for PlayerListPacketType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Add,
            1 => Self::Remove,
            value => Self::Unknown(value),
        }
    }
}

impl PlayerListPacketType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Add => 0,
            Self::Remove => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<PlayerListPacketType> for u8 {
    fn from(value: PlayerListPacketType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for PlayerListPacketType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PlayerListPacketType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PlayerLocationType {
    #[default]
    PlayerLocationCoordinates,
    PlayerLocationHide,
    Unknown(i32),
}

impl From<i32> for PlayerLocationType {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::PlayerLocationCoordinates,
            1 => Self::PlayerLocationHide,
            value => Self::Unknown(value),
        }
    }
}

impl PlayerLocationType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::PlayerLocationCoordinates => 0,
            Self::PlayerLocationHide => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<PlayerLocationType> for i32 {
    fn from(value: PlayerLocationType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for PlayerLocationType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PlayerLocationType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PlayerPermissionLevel {
    #[default]
    Visitor,
    Member,
    Operator,
    Custom,
    Unknown(i8),
}

impl From<i8> for PlayerPermissionLevel {
    fn from(value: i8) -> Self {
        match value {
            0 => Self::Visitor,
            1 => Self::Member,
            2 => Self::Operator,
            3 => Self::Custom,
            value => Self::Unknown(value),
        }
    }
}

impl PlayerPermissionLevel {
    pub fn to_raw(self) -> i8 {
        match self {
            Self::Visitor => 0,
            Self::Member => 1,
            Self::Operator => 2,
            Self::Custom => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<PlayerPermissionLevel> for i8 {
    fn from(value: PlayerPermissionLevel) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for PlayerPermissionLevel {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::I8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PlayerPermissionLevel {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::I8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PlayerPositionModeComponentPositionMode {
    #[default]
    Normal,
    Respawn,
    Teleport,
    Onlyheadrot,
    Unknown(u8),
}

impl From<u8> for PlayerPositionModeComponentPositionMode {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Normal,
            1 => Self::Respawn,
            2 => Self::Teleport,
            3 => Self::Onlyheadrot,
            value => Self::Unknown(value),
        }
    }
}

impl PlayerPositionModeComponentPositionMode {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Normal => 0,
            Self::Respawn => 1,
            Self::Teleport => 2,
            Self::Onlyheadrot => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<PlayerPositionModeComponentPositionMode> for u8 {
    fn from(value: PlayerPositionModeComponentPositionMode) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for PlayerPositionModeComponentPositionMode {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PlayerPositionModeComponentPositionMode {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PlayerRespawnState {
    #[default]
    Searchingforspawn,
    Readytospawn,
    Clientreadytospawn,
    Unknown(u8),
}

impl From<u8> for PlayerRespawnState {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Searchingforspawn,
            1 => Self::Readytospawn,
            2 => Self::Clientreadytospawn,
            value => Self::Unknown(value),
        }
    }
}

impl PlayerRespawnState {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Searchingforspawn => 0,
            Self::Readytospawn => 1,
            Self::Clientreadytospawn => 2,
            Self::Unknown(value) => value,
        }
    }
}

impl From<PlayerRespawnState> for u8 {
    fn from(value: PlayerRespawnState) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for PlayerRespawnState {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PlayerRespawnState {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

// Domain: position_tracking

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PositionTrackingDBClientRequestAction {
    #[default]
    Query,
    Unknown(u8),
}

impl From<u8> for PositionTrackingDBClientRequestAction {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Query,
            value => Self::Unknown(value),
        }
    }
}

impl PositionTrackingDBClientRequestAction {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Query => 0,
            Self::Unknown(value) => value,
        }
    }
}

impl From<PositionTrackingDBClientRequestAction> for u8 {
    fn from(value: PositionTrackingDBClientRequestAction) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for PositionTrackingDBClientRequestAction {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PositionTrackingDBClientRequestAction {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PositionTrackingDBServerBroadcastAction {
    #[default]
    Update,
    Destroy,
    Notfound,
    Unknown(u8),
}

impl From<u8> for PositionTrackingDBServerBroadcastAction {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Update,
            1 => Self::Destroy,
            2 => Self::Notfound,
            value => Self::Unknown(value),
        }
    }
}

impl PositionTrackingDBServerBroadcastAction {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Update => 0,
            Self::Destroy => 1,
            Self::Notfound => 2,
            Self::Unknown(value) => value,
        }
    }
}

impl From<PositionTrackingDBServerBroadcastAction> for u8 {
    fn from(value: PositionTrackingDBServerBroadcastAction) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for PositionTrackingDBServerBroadcastAction {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PositionTrackingDBServerBroadcastAction {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

// Domain: recipe

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum RecipeUnlockingRequirementUnlockingContext {
    #[default]
    None,
    Alwaysunlocked,
    Playerinwater,
    Playerhasmanyitems,
    Unknown(i32),
}

impl From<i32> for RecipeUnlockingRequirementUnlockingContext {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Alwaysunlocked,
            2 => Self::Playerinwater,
            3 => Self::Playerhasmanyitems,
            value => Self::Unknown(value),
        }
    }
}

impl RecipeUnlockingRequirementUnlockingContext {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::None => 0,
            Self::Alwaysunlocked => 1,
            Self::Playerinwater => 2,
            Self::Playerhasmanyitems => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<RecipeUnlockingRequirementUnlockingContext> for i32 {
    fn from(value: RecipeUnlockingRequirementUnlockingContext) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for RecipeUnlockingRequirementUnlockingContext {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for RecipeUnlockingRequirementUnlockingContext {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

// Domain: scoreboard

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ScoreboardIdentityPacketType {
    #[default]
    Update,
    Remove,
    Unknown(u8),
}

impl From<u8> for ScoreboardIdentityPacketType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Update,
            1 => Self::Remove,
            value => Self::Unknown(value),
        }
    }
}

impl ScoreboardIdentityPacketType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Update => 0,
            Self::Remove => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ScoreboardIdentityPacketType> for u8 {
    fn from(value: ScoreboardIdentityPacketType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ScoreboardIdentityPacketType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ScoreboardIdentityPacketType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

// Domain: skin

/// PersonaPiece represents a piece of a persona skin. All pieces are sent separately.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PersonaPieceType {
    #[default]
    Unknown,
    Skeleton,
    Body,
    Skin,
    Bottom,
    Feet,
    Dress,
    Top,
    HighPants,
    Hands,
    Outerwear,
    Facialhair,
    Mouth,
    Eyes,
    Hair,
    Hood,
    Back,
    Faceaccessory,
    Head,
    Legs,
    Leftleg,
    Rightleg,
    Arms,
    Leftarm,
    Rightarm,
    Capes,
    Classicskin,
    Emote,
    Unsupported,
    Unknown2(u32),
}

impl From<u32> for PersonaPieceType {
    fn from(value: u32) -> Self {
        match value {
            0 => Self::Unknown,
            1 => Self::Skeleton,
            2 => Self::Body,
            3 => Self::Skin,
            4 => Self::Bottom,
            5 => Self::Feet,
            6 => Self::Dress,
            7 => Self::Top,
            8 => Self::HighPants,
            9 => Self::Hands,
            10 => Self::Outerwear,
            11 => Self::Facialhair,
            12 => Self::Mouth,
            13 => Self::Eyes,
            14 => Self::Hair,
            15 => Self::Hood,
            16 => Self::Back,
            17 => Self::Faceaccessory,
            18 => Self::Head,
            19 => Self::Legs,
            20 => Self::Leftleg,
            21 => Self::Rightleg,
            22 => Self::Arms,
            23 => Self::Leftarm,
            24 => Self::Rightarm,
            25 => Self::Capes,
            26 => Self::Classicskin,
            27 => Self::Emote,
            28 => Self::Unsupported,
            value => Self::Unknown2(value),
        }
    }
}

impl PersonaPieceType {
    pub fn to_raw(self) -> u32 {
        match self {
            Self::Unknown => 0,
            Self::Skeleton => 1,
            Self::Body => 2,
            Self::Skin => 3,
            Self::Bottom => 4,
            Self::Feet => 5,
            Self::Dress => 6,
            Self::Top => 7,
            Self::HighPants => 8,
            Self::Hands => 9,
            Self::Outerwear => 10,
            Self::Facialhair => 11,
            Self::Mouth => 12,
            Self::Eyes => 13,
            Self::Hair => 14,
            Self::Hood => 15,
            Self::Back => 16,
            Self::Faceaccessory => 17,
            Self::Head => 18,
            Self::Legs => 19,
            Self::Leftleg => 20,
            Self::Rightleg => 21,
            Self::Arms => 22,
            Self::Leftarm => 23,
            Self::Rightarm => 24,
            Self::Capes => 25,
            Self::Classicskin => 26,
            Self::Emote => 27,
            Self::Unsupported => 28,
            Self::Unknown2(value) => value,
        }
    }
}

impl From<PersonaPieceType> for u32 {
    fn from(value: PersonaPieceType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for PersonaPieceType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U32LE(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PersonaPieceType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U32LE as wire::Decode>::decode(reader)?.0))
    }
}

// Domain: structure

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum StructureBlockType {
    #[default]
    Data,
    Save,
    Load,
    Corner,
    Invalid,
    Export,
    Unknown(i32),
}

impl From<i32> for StructureBlockType {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Data,
            1 => Self::Save,
            2 => Self::Load,
            3 => Self::Corner,
            4 => Self::Invalid,
            5 => Self::Export,
            value => Self::Unknown(value),
        }
    }
}

impl StructureBlockType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Data => 0,
            Self::Save => 1,
            Self::Load => 2,
            Self::Corner => 3,
            Self::Invalid => 4,
            Self::Export => 5,
            Self::Unknown(value) => value,
        }
    }
}

impl From<StructureBlockType> for i32 {
    fn from(value: StructureBlockType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for StructureBlockType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for StructureBlockType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum StructureRedstoneSaveMode {
    #[default]
    Savestomemory,
    Savestodisk,
    Unknown(u8),
}

impl From<u8> for StructureRedstoneSaveMode {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Savestomemory,
            1 => Self::Savestodisk,
            value => Self::Unknown(value),
        }
    }
}

impl StructureRedstoneSaveMode {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Savestomemory => 0,
            Self::Savestodisk => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<StructureRedstoneSaveMode> for u8 {
    fn from(value: StructureRedstoneSaveMode) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for StructureRedstoneSaveMode {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for StructureRedstoneSaveMode {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum StructureTemplateRequestOperation {
    #[default]
    None,
    Exportfromsavemode,
    Exportfromloadmode,
    Querysavedstructure,
    Unknown(u8),
}

impl From<u8> for StructureTemplateRequestOperation {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Exportfromsavemode,
            2 => Self::Exportfromloadmode,
            3 => Self::Querysavedstructure,
            value => Self::Unknown(value),
        }
    }
}

impl StructureTemplateRequestOperation {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Exportfromsavemode => 1,
            Self::Exportfromloadmode => 2,
            Self::Querysavedstructure => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<StructureTemplateRequestOperation> for u8 {
    fn from(value: StructureTemplateRequestOperation) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for StructureTemplateRequestOperation {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for StructureTemplateRequestOperation {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum StructureTemplateResponseType {
    #[default]
    None,
    Export,
    Query,
    Unknown(u8),
}

impl From<u8> for StructureTemplateResponseType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Export,
            2 => Self::Query,
            value => Self::Unknown(value),
        }
    }
}

impl StructureTemplateResponseType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Export => 1,
            Self::Query => 2,
            Self::Unknown(value) => value,
        }
    }
}

impl From<StructureTemplateResponseType> for u8 {
    fn from(value: StructureTemplateResponseType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for StructureTemplateResponseType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for StructureTemplateResponseType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

// Domain: sub_chunk

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum SubChunkRequestResult {
    #[default]
    Success,
    Levelchunkdoesntexist,
    Wrongdimension,
    Playerdoesntexist,
    Indexoutofbounds,
    Successallair,
    Unknown(u8),
}

impl From<u8> for SubChunkRequestResult {
    fn from(value: u8) -> Self {
        match value {
            1 => Self::Success,
            2 => Self::Levelchunkdoesntexist,
            3 => Self::Wrongdimension,
            4 => Self::Playerdoesntexist,
            5 => Self::Indexoutofbounds,
            6 => Self::Successallair,
            value => Self::Unknown(value),
        }
    }
}

impl SubChunkRequestResult {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Success => 1,
            Self::Levelchunkdoesntexist => 2,
            Self::Wrongdimension => 3,
            Self::Playerdoesntexist => 4,
            Self::Indexoutofbounds => 5,
            Self::Successallair => 6,
            Self::Unknown(value) => value,
        }
    }
}

impl From<SubChunkRequestResult> for u8 {
    fn from(value: SubChunkRequestResult) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for SubChunkRequestResult {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SubChunkRequestResult {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

// Domain: telemetry

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum LegacyTelemetryType {
    #[default]
    Achievement,
    Interaction,
    Portalcreated,
    Portalused,
    Mobkilled,
    Cauldronused,
    Playerdied,
    Bosskilled,
    AgentcommandObsolete,
    Agentcreated,
    PatternremovedObsolete,
    Slashcommand,
    FishbucketedObsolete,
    Mobborn,
    PetdiedObsolete,
    Poicauldronused,
    Composterused,
    Bellused,
    Actordefinition,
    Raidupdate,
    PlayermovementanomalyObsolete,
    PlayermovementcorrectedObsolete,
    Honeyharvested,
    Targetblockhit,
    Piglinbarter,
    Playerwaxedorunwaxedcopper,
    Codebuilderruntimeaction,
    Codebuilderscoreboard,
    Striderriddeninlavainoverworld,
    Sneakclosetosculksensor,
    Carefulrestoration,
    Itemused,
    Unknown(i32),
}

impl From<i32> for LegacyTelemetryType {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Achievement,
            1 => Self::Interaction,
            2 => Self::Portalcreated,
            3 => Self::Portalused,
            4 => Self::Mobkilled,
            5 => Self::Cauldronused,
            6 => Self::Playerdied,
            7 => Self::Bosskilled,
            8 => Self::AgentcommandObsolete,
            9 => Self::Agentcreated,
            10 => Self::PatternremovedObsolete,
            11 => Self::Slashcommand,
            12 => Self::FishbucketedObsolete,
            13 => Self::Mobborn,
            14 => Self::PetdiedObsolete,
            15 => Self::Poicauldronused,
            16 => Self::Composterused,
            17 => Self::Bellused,
            18 => Self::Actordefinition,
            19 => Self::Raidupdate,
            20 => Self::PlayermovementanomalyObsolete,
            21 => Self::PlayermovementcorrectedObsolete,
            22 => Self::Honeyharvested,
            23 => Self::Targetblockhit,
            24 => Self::Piglinbarter,
            25 => Self::Playerwaxedorunwaxedcopper,
            26 => Self::Codebuilderruntimeaction,
            27 => Self::Codebuilderscoreboard,
            28 => Self::Striderriddeninlavainoverworld,
            29 => Self::Sneakclosetosculksensor,
            30 => Self::Carefulrestoration,
            31 => Self::Itemused,
            value => Self::Unknown(value),
        }
    }
}

impl LegacyTelemetryType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Achievement => 0,
            Self::Interaction => 1,
            Self::Portalcreated => 2,
            Self::Portalused => 3,
            Self::Mobkilled => 4,
            Self::Cauldronused => 5,
            Self::Playerdied => 6,
            Self::Bosskilled => 7,
            Self::AgentcommandObsolete => 8,
            Self::Agentcreated => 9,
            Self::PatternremovedObsolete => 10,
            Self::Slashcommand => 11,
            Self::FishbucketedObsolete => 12,
            Self::Mobborn => 13,
            Self::PetdiedObsolete => 14,
            Self::Poicauldronused => 15,
            Self::Composterused => 16,
            Self::Bellused => 17,
            Self::Actordefinition => 18,
            Self::Raidupdate => 19,
            Self::PlayermovementanomalyObsolete => 20,
            Self::PlayermovementcorrectedObsolete => 21,
            Self::Honeyharvested => 22,
            Self::Targetblockhit => 23,
            Self::Piglinbarter => 24,
            Self::Playerwaxedorunwaxedcopper => 25,
            Self::Codebuilderruntimeaction => 26,
            Self::Codebuilderscoreboard => 27,
            Self::Striderriddeninlavainoverworld => 28,
            Self::Sneakclosetosculksensor => 29,
            Self::Carefulrestoration => 30,
            Self::Itemused => 31,
            Self::Unknown(value) => value,
        }
    }
}

impl From<LegacyTelemetryType> for i32 {
    fn from(value: LegacyTelemetryType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for LegacyTelemetryType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for LegacyTelemetryType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

// Domain: text

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum TextPacketType {
    #[default]
    Raw,
    Chat,
    Translate,
    Popup,
    Jukeboxpopup,
    Tip,
    Systemmessage,
    Whisper,
    Announcement,
    Textobjectwhisper,
    Textobject,
    Textobjectannouncement,
    Unknown(u8),
}

impl From<u8> for TextPacketType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Raw,
            1 => Self::Chat,
            2 => Self::Translate,
            3 => Self::Popup,
            4 => Self::Jukeboxpopup,
            5 => Self::Tip,
            6 => Self::Systemmessage,
            7 => Self::Whisper,
            8 => Self::Announcement,
            9 => Self::Textobjectwhisper,
            10 => Self::Textobject,
            11 => Self::Textobjectannouncement,
            value => Self::Unknown(value),
        }
    }
}

impl TextPacketType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Raw => 0,
            Self::Chat => 1,
            Self::Translate => 2,
            Self::Popup => 3,
            Self::Jukeboxpopup => 4,
            Self::Tip => 5,
            Self::Systemmessage => 6,
            Self::Whisper => 7,
            Self::Announcement => 8,
            Self::Textobjectwhisper => 9,
            Self::Textobject => 10,
            Self::Textobjectannouncement => 11,
            Self::Unknown(value) => value,
        }
    }
}

impl From<TextPacketType> for u8 {
    fn from(value: TextPacketType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for TextPacketType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for TextPacketType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum TextProcessingEventOrigin {
    #[default]
    Unknown,
    Serverchatpublic,
    Serverchatwhisper,
    Signtext,
    Anviltext,
    Bookandquilltext,
    Commandblocktext,
    Blockactordatatext,
    Joineventtext,
    Leaveeventtext,
    Slashcommandchat,
    Cartographytext,
    Kickcommand,
    Titlecommand,
    Summoncommand,
    Serverform,
    Datadrivenui,
    Unknown2(i32),
}

impl From<i32> for TextProcessingEventOrigin {
    fn from(value: i32) -> Self {
        match value {
            -1 => Self::Unknown,
            0 => Self::Serverchatpublic,
            1 => Self::Serverchatwhisper,
            2 => Self::Signtext,
            3 => Self::Anviltext,
            4 => Self::Bookandquilltext,
            5 => Self::Commandblocktext,
            6 => Self::Blockactordatatext,
            7 => Self::Joineventtext,
            8 => Self::Leaveeventtext,
            9 => Self::Slashcommandchat,
            10 => Self::Cartographytext,
            11 => Self::Kickcommand,
            12 => Self::Titlecommand,
            13 => Self::Summoncommand,
            14 => Self::Serverform,
            15 => Self::Datadrivenui,
            value => Self::Unknown2(value),
        }
    }
}

impl TextProcessingEventOrigin {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Unknown => -1,
            Self::Serverchatpublic => 0,
            Self::Serverchatwhisper => 1,
            Self::Signtext => 2,
            Self::Anviltext => 3,
            Self::Bookandquilltext => 4,
            Self::Commandblocktext => 5,
            Self::Blockactordatatext => 6,
            Self::Joineventtext => 7,
            Self::Leaveeventtext => 8,
            Self::Slashcommandchat => 9,
            Self::Cartographytext => 10,
            Self::Kickcommand => 11,
            Self::Titlecommand => 12,
            Self::Summoncommand => 13,
            Self::Serverform => 14,
            Self::Datadrivenui => 15,
            Self::Unknown2(value) => value,
        }
    }
}

impl From<TextProcessingEventOrigin> for i32 {
    fn from(value: TextProcessingEventOrigin) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for TextProcessingEventOrigin {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::I32LE(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for TextProcessingEventOrigin {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::I32LE as wire::Decode>::decode(reader)?.0))
    }
}
