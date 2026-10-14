package main

import (
	"fmt"
	"strings"
	"testing"
)

func TestHistoricalFamilyNamesMatchPinnedRegistries(t *testing.T) {
	names := make(map[string]struct{})
	for _, target := range []struct {
		version  string
		protocol uint32
	}{
		{"v1001", registryProtocol}, {"v2193", v2193BlockProtocol},
	} {
		data, err := readAssetData(".", fmt.Sprintf("block-registry-%s.bin", target.version))
		if err != nil {
			t.Fatal(err)
		}
		_, records, err := decodeBREGRecords(data, target.protocol)
		if err != nil {
			t.Fatal(err)
		}
		for _, record := range records {
			if record.Name != "cinnabar:reserved" {
				names[record.Name] = struct{}{}
			}
		}
	}
	states, err := readV2193BlockStates()
	if err != nil {
		t.Fatal(err)
	}
	for _, state := range states {
		names[state.Name] = struct{}{}
	}
	for name := range names {
		got, err := legacyModelFamily(sourceState(name))
		if err != nil {
			t.Fatal(err)
		}
		if want := historicalNameFamily(name); got != want {
			t.Errorf("%s family = %v, want %v", name, got, want)
		}
	}
}

func TestHistoricalFamilyCompatibilityNames(t *testing.T) {
	for _, name := range []string{
		"minecraft:tall_seagrass", "minecraft:kelp_plant", "minecraft:fence", "minecraft:carpet",
	} {
		got, err := classifyRecord(sourceState(name))
		if err != nil {
			t.Fatal(err)
		}
		if want := historicalNameFamily(name); got.ModelFamily != want {
			t.Errorf("%s family = %v, want %v", name, got.ModelFamily, want)
		}
	}
}

// historicalNameFamily preserves the name-only family classification contract.
func historicalNameFamily(name string) ModelFamily {
	name = strings.TrimPrefix(name, "minecraft:")
	switch {
	case name == "air":
		return ModelFamilyAir
	case name == "water" || name == "flowing_water" || name == "lava" || name == "flowing_lava":
		return ModelFamilyLiquid
	case strings.Contains(name, "copper_golem_statue"):
		return ModelFamilyStatue
	case name == "barrier":
		return ModelFamilyInvisible
	case name == "dragon_egg":
		return ModelFamilyDecorative
	case name == "soul_sand" || name == "mud":
		return ModelFamilyCuboid
	case fixtureIsReviewedSelectorAliasCubeName(name), fixtureIsGlazedTerracottaName(name):
		return ModelFamilyCube
	case strings.Contains(name, "trapdoor"):
		return ModelFamilyTrapdoor
	case strings.HasSuffix(name, "_door") || name == "wooden_door":
		return ModelFamilyDoor
	case strings.HasSuffix(name, "_stairs"):
		return ModelFamilyStair
	case strings.Contains(name, "slab"):
		return ModelFamilySlab
	case strings.Contains(name, "fence_gate"):
		return ModelFamilyGate
	case strings.HasSuffix(name, "_wall") || name == "cobblestone_wall":
		return ModelFamilyWall
	case strings.HasSuffix(name, "_fence") || name == "fence" || name == "nether_brick_fence":
		return ModelFamilyFence
	case strings.Contains(name, "glass_pane") || strings.HasSuffix(name, "_pane") || strings.HasSuffix(name, "_bars"):
		return ModelFamilyPane
	case strings.HasSuffix(name, "_bed") || name == "bed":
		return ModelFamilyBed
	case strings.Contains(name, "chest"):
		return ModelFamilyChest
	case strings.Contains(name, "sign"):
		return ModelFamilySign
	case strings.Contains(name, "rail"):
		return ModelFamilyRail
	case fixtureIsTorchName(name):
		return ModelFamilyTorch
	case name == "lever":
		return ModelFamilyLever
	case strings.HasSuffix(name, "_button") || name == "stone_button":
		return ModelFamilyButton
	case strings.Contains(name, "pressure_plate"):
		return ModelFamilyPressurePlate
	case strings.HasSuffix(name, "_carpet") || name == "carpet":
		return ModelFamilyCarpet
	case name == "snow_layer" || name == "leaf_litter":
		return ModelFamilyLayer
	case fixtureIsAquaticName(name):
		return ModelFamilyAquatic
	case name == "cocoa":
		return ModelFamilyCocoa
	case fixtureIsCropName(name):
		return ModelFamilyCrop
	case name == "wildflowers" || name == "pink_petals":
		return ModelFamilyFlowerBed
	case name == "vine":
		return ModelFamilyVine
	case name == "glow_lichen":
		return ModelFamilyGlowLichen
	case name == "sculk_vein":
		return ModelFamilySculkVein
	case name == "chiseled_bookshelf":
		return ModelFamilyChiseledBookshelf
	case name == "bee_nest" || name == "beehive":
		return ModelFamilyCube
	case name == "resin_clump":
		return ModelFamilyResinClump
	case name == "cactus":
		return ModelFamilyCuboid
	case name == "cake":
		return ModelFamilyCuboid
	case name == "farmland":
		return ModelFamilyCuboid
	case fixtureIsShelfName(name):
		return ModelFamilyCuboid
	case fixtureIsCrossName(name):
		return ModelFamilyCross
	}
	return ModelFamilyUnknown
}

// fixtureIsShelfName preserves the historical name predicate.
func fixtureIsShelfName(name string) bool {
	name = strings.TrimPrefix(name, "minecraft:")
	switch name {
	case "acacia_shelf", "bamboo_shelf", "birch_shelf", "cherry_shelf",
		"crimson_shelf", "dark_oak_shelf", "jungle_shelf", "mangrove_shelf",
		"oak_shelf", "pale_oak_shelf", "spruce_shelf", "warped_shelf":
		return true
	default:
		return false
	}
}

// fixtureIsCropName preserves the historical name predicate.
func fixtureIsCropName(name string) bool {
	switch name {
	case "wheat", "carrots", "potatoes", "beetroot", "nether_wart", "sweet_berry_bush", "torchflower_crop", "pitcher_crop", "melon_stem", "pumpkin_stem":
		return true
	default:
		return false
	}
}

// fixtureIsCrossName preserves the historical name predicate.
func fixtureIsCrossName(name string) bool {
	if name == "chorus_flower" {
		return false
	}
	if name == "short_grass" || name == "tall_grass" || name == "short_dry_grass" || name == "tall_dry_grass" || name == "fern" || name == "large_fern" || name == "deadbush" || name == "bush" || name == "red_flower" || name == "yellow_flower" || name == "dandelion" || name == "poppy" || name == "blue_orchid" || name == "allium" || name == "azure_bluet" || name == "oxeye_daisy" || name == "cornflower" || name == "lily_of_the_valley" || name == "wither_rose" || name == "sunflower" || name == "lilac" || name == "rose_bush" || name == "peony" || name == "brown_mushroom" || name == "red_mushroom" || name == "crimson_fungus" || name == "warped_fungus" || name == "crimson_roots" || name == "warped_roots" || name == "nether_sprouts" || name == "mangrove_propagule" || name == "hanging_roots" || name == "pale_hanging_moss" || name == "firefly_bush" || name == "reeds" || name == "weeping_vines" || name == "twisting_vines" || strings.HasPrefix(name, "cave_vines") || name == "web" || name == "fire" || name == "soul_fire" || name == "torchflower" {
		return true
	}
	return !strings.Contains(name, "flower_pot") && (strings.HasSuffix(name, "_flower") || strings.HasSuffix(name, "_sapling"))
}

// fixtureIsAquaticName preserves the historical name predicate.
func fixtureIsAquaticName(name string) bool {
	return name == "seagrass" || name == "tall_seagrass" || name == "kelp" || name == "kelp_plant" || (strings.Contains(name, "coral") && !strings.Contains(name, "coral_block"))
}

// fixtureIsTorchName preserves the historical name predicate.
func fixtureIsTorchName(name string) bool {
	return name == "torch" || name == "copper_torch" || name == "soul_torch" || name == "redstone_torch" || name == "unlit_redstone_torch"
}

// fixtureIsGlazedTerracottaName preserves the historical name predicate.
func fixtureIsGlazedTerracottaName(name string) bool {
	return strings.HasSuffix(name, "_glazed_terracotta")
}

// fixtureIsReviewedSelectorAliasCubeName preserves the historical name predicate.
func fixtureIsReviewedSelectorAliasCubeName(name string) bool {
	switch name {
	case "bone_block", "hay_block", "chiseled_quartz_block", "purpur_block", "quartz_block", "smooth_quartz", "tnt":
		return true
	default:
		return false
	}
}
