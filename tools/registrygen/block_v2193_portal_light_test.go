package main

import (
	"fmt"
	"testing"
)

func TestPortalNativeLightProperties(t *testing.T) {
	t.Parallel()
	for _, axis := range []struct{ name string }{
		{name: "unknown"},
		{name: "x"},
		{name: "z"},
	} {
		t.Run(axis.name, func(t *testing.T) {
			t.Parallel()
			records := []Record{{
				Name: "minecraft:portal",
				StateJSON: []byte(fmt.Sprintf(
					`{"portal_axis":{"type":"string","value":%q}}`, axis.name)),
			}}
			for _, source := range []struct {
				name       string
				properties byte
			}{
				{name: "unimplemented legacy", properties: 0xf0},
				{name: "incorrect retail", properties: 0xfb},
				{name: "native", properties: 0x0b},
				{name: "constructor without emission", properties: 0},
			} {
				t.Run(source.name, func(t *testing.T) {
					t.Parallel()
					properties := []byte{source.properties}
					// The identified client contract takes precedence over this table.
					retail := map[string]PMMPLightProperties{
						"minecraft:portal": {Brightness: 11, Opacity: 1},
					}
					changed, err := applyRetailLightCorrections(records, properties, retail)
					if err != nil {
						t.Fatal(err)
					}
					if got := properties[0]; got != 0x0b {
						t.Fatalf("native portal light = %#x, want emission 11/filter 0", got)
					}
					var wantChanged int
					if source.properties != 0x0b {
						wantChanged = 1
					}
					if changed != wantChanged {
						t.Fatalf("changed = %d, want %d", changed, wantChanged)
					}
					changed, err = applyRetailLightCorrections(records, properties, retail)
					if err != nil || changed != 0 {
						t.Fatalf("repeat correction changed=%d err=%v", changed, err)
					}
				})
			}
		})
	}
}
