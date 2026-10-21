//! TextKey names used by UI consumers.

names! {
    TextKey {
        ContainerBarrel = "container.barrel",
        ContainerBeacon = "container.beacon",
        ContainerBrewing = "container.brewing",
        ContainerChest = "container.chest",
        ContainerChestDouble = "container.chestDouble",
        ContainerCrafter = "container.crafter",
        ContainerCrafting = "container.crafting",
        ContainerDispenser = "container.dispenser",
        ContainerDropper = "container.dropper",
        ContainerEnchant = "container.enchant",
        ContainerEnderchest = "container.enderchest",
        ContainerFurnace = "container.furnace",
        ContainerHopper = "container.hopper",
        ContainerInventory = "container.inventory",
        ContainerLoom = "container.loom",
        ContainerRepair = "container.repair",
        ContainerShulkerbox = "container.shulkerbox",
        ContainerSmithingTable = "container.smithing_table",
        ContainerStonecutter = "container.stonecutter",
        EntityHorseName = "entity.horse.name",
        TileBeaconName = "tile.beacon.name",
        TileBlastFurnaceName = "tile.blast_furnace.name",
        TileSmokerName = "tile.smoker.name",
        TradeTrade = "trade.trade",
    }
}

impl TextKey {
    /// Returns the localized level name for a zero-based trading tier.
    pub fn trade_level(tier: i32) -> String {
        format!("trade.level.{}", i64::from(tier) + 1)
    }
}
