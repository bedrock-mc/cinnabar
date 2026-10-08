package main

import (
	"fmt"
	"testing"
)

// Current native registration uses BaseGameVersion, not liquid_depth, to
// select still-water dampening one. Dynamic/flowing water stays at two.
func TestWaterNativeLightDampening(t *testing.T) {
	t.Parallel()
	for _, c := range []struct {
		name   string
		filter byte
	}{
		{name: "minecraft:water", filter: 1},
		{name: "minecraft:flowing_water", filter: 2},
	} {
		for depth := range 16 {
			t.Run(fmt.Sprintf("%s/%d", c.name, depth), func(t *testing.T) {
				t.Parallel()
				records := []Record{{
					Name: c.name,
					StateJSON: []byte(fmt.Sprintf(
						`{"liquid_depth":{"type":"int","value":%d}}`, depth)),
				}}
				// Exercise stale constructor/table values and preserve the
				// independent emission nibble, even for unusual input.
				for _, current := range []byte{0, 2 << 4, 15 << 4, 15<<4 | 9} {
					properties := []byte{current}
					retail := map[string]PMMPLightProperties{c.name: {Opacity: 15}}
					changed, err := applyRetailLightCorrections(records, properties, retail)
					if err != nil {
						t.Fatal(err)
					}
					want := current&0x0f | c.filter<<4
					if properties[0] != want {
						t.Fatalf("input=%#x got=%#x want=%#x", current, properties[0], want)
					}
					wantChanged := 0
					if current != want {
						wantChanged = 1
					}
					if changed != wantChanged {
						t.Fatalf("changed=%d want=%d", changed, wantChanged)
					}
					changed, err = applyRetailLightCorrections(records, properties, retail)
					if err != nil || changed != 0 {
						t.Fatalf("repeat correction changed=%d err=%v", changed, err)
					}
				}
			})
		}
	}
}
