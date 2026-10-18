package main

import (
	"encoding/json"
	"testing"

	sharedblock "github.com/cinnabar-mc/protocolgen/generated/data/block"
)

// TestBlockFactExportsLiquidContainment keeps source booleans intact across the language boundary.
func TestBlockFactExportsLiquidContainment(t *testing.T) {
	for _, value := range []bool{false, true} {
		fact := blockFact(Record{}, sharedblock.Properties{CanContainLiquidSource: value}, nil)
		if got, ok := fact["can_contain_liquid_source"].(bool); !ok || got != value {
			t.Fatalf("liquid containment = %v, want %v", fact["can_contain_liquid_source"], value)
		}
	}
}

// TestBlockFactKeepsExactFloat32Values covers float32 values shortened by JSON, which shifts fixed-point collision and friction.
func TestBlockFactKeepsExactFloat32Values(t *testing.T) {
	fact := blockFact(Record{}, sharedblock.Properties{Friction: 0.6}, []sharedblock.Box{{0, 0, 0, 0.975, 1, 1}})
	data, err := json.Marshal(fact)
	if err != nil {
		t.Fatal(err)
	}
	var got struct {
		Boxes    [][6]float64
		Friction float64
	}
	if err := json.Unmarshal(data, &got); err != nil {
		t.Fatal(err)
	}
	if got.Boxes[0][3] != 0.9750000238418579 {
		t.Fatalf("box coordinate = %v", got.Boxes[0][3])
	}
	if got.Friction != 0.6000000238418579 {
		t.Fatalf("friction = %v", got.Friction)
	}
}
