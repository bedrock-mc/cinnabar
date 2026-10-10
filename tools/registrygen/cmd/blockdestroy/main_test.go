package main

import (
	"reflect"
	"testing"
)

// TestBuildJoinsSharedMiningFacts checks known tool families and unknown rows.
func TestBuildJoinsSharedMiningFacts(t *testing.T) {
	hardness := map[string]float32{
		"minecraft:obsidian":    35,
		"minecraft:grass_block": 0.6,
		"minecraft:web":         4,
		"minecraft:custom_only": 1,
		"minecraft:cocoa":       0.2,
		"minecraft:glass":       0.3,
	}
	rows := build(hardness)
	want := []row{
		{"minecraft:cocoa", 0.2, nil, nil, -1, true},
		{"minecraft:custom_only", 1, nil, nil, -1, true},
		{"minecraft:glass", 0.3, nil, nil, -1, true},
		{"minecraft:grass_block", 0.6, []string{"shovel"}, nil, -1, false},
		{"minecraft:obsidian", 35, []string{"pickaxe"}, []string{"pickaxe"}, 3, false},
		{"minecraft:web", 4, []string{"shears", "sword"}, []string{"shears", "sword"}, 0, false},
	}
	if !reflect.DeepEqual(rows, want) {
		t.Fatalf("rows = %#v\nwant %#v", rows, want)
	}
}

// TestSharedHardnessPreservesEveryBlock checks that the name-keyed table can represent this catalog.
func TestSharedHardnessPreservesEveryBlock(t *testing.T) {
	values, err := sharedHardness()
	if err != nil {
		t.Fatal(err)
	}
	if len(values) != 1477 || values["minecraft:obsidian"] != 35 {
		t.Fatalf("unexpected shared hardness catalog: %d names, obsidian=%v", len(values), values["minecraft:obsidian"])
	}
}
