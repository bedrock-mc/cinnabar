package main

import (
	"fmt"
	"testing"
)

// Only an unimplemented-block default is replaced; known values and unlisted names stay.
func TestRetailLightCorrectionsReplaceOnlyUnimplementedDefaults(t *testing.T) {
	records := []Record{
		{Name: "minecraft:glow_lichen"},
		{Name: "minecraft:vine"},
		{Name: "minecraft:stone"},
		{Name: "minecraft:torch"},
		{Name: "test:unlisted"},
	}
	properties := []byte{15 << 4, 15 << 4, 15 << 4, 14, 15 << 4}
	retail := map[string]PMMPLightProperties{
		"minecraft:glow_lichen": {Brightness: 7, Opacity: 0},
		"minecraft:vine":        {Brightness: 0, Opacity: 0},
		"minecraft:stone":       {Brightness: 0, Opacity: 1},
		"minecraft:torch":       {Brightness: 0, Opacity: 0},
	}
	changed, err := applyRetailLightCorrections(records, properties, retail)
	if err != nil {
		t.Fatal(err)
	}
	want := []byte{7, 0, 15 << 4, 14, 15 << 4}
	for index := range want {
		if properties[index] != want[index] {
			t.Fatalf("state %d = %#x, want %#x", index, properties[index], want[index])
		}
	}
	if changed != 2 {
		t.Fatalf("changed = %d", changed)
	}
}

func TestSensorLightKeepsItsIdentifiedFilterWhenPhaseControlsEmission(t *testing.T) {
	for _, name := range []string{"minecraft:sculk_sensor", "minecraft:calibrated_sculk_sensor"} {
		records := []Record{{Name: name, StateJSON: []byte(`{"sculk_sensor_phase":{"type":"int","value":1}}`)}}
		properties := []byte{15 << 4}
		sources := map[string]PMMPLightProperties{name: {Opacity: 0.19999998807907104}}
		if _, err := applyRetailLightCorrections(records, properties, sources); err != nil {
			t.Fatal(err)
		}
		if properties[0] != 3<<4|1 {
			t.Fatalf("sensor phase replaced its light filter: %#x", properties[0])
		}
	}
}

// 1.26.50.26 snow layers have zero light dampening at every height.
func TestSnowLayerNativeLightDampening(t *testing.T) {
	var records []Record
	var properties []byte
	for height := range 8 {
		for covered := range 2 {
			records = append(records, Record{
				Name:      "minecraft:snow_layer",
				StateJSON: []byte(fmt.Sprintf(`{"height":{"type":"int","value":%d},"covered_bit":{"type":"byte","value":%d}}`, height, covered)),
			})
			properties = append(properties, 2<<4)
		}
	}
	records = append(records, Record{Name: "minecraft:leaves"}, Record{Name: "test:snow_layer"})
	properties = append(properties, 2<<4, 2<<4)
	retail := map[string]PMMPLightProperties{
		"minecraft:snow_layer": {Opacity: 2},
	}
	changed, err := applyRetailLightCorrections(records, properties, retail)
	if err != nil {
		t.Fatal(err)
	}
	for index, record := range records {
		want := byte(2 << 4)
		if record.Name == "minecraft:snow_layer" {
			want = 0
		}
		if got := properties[index]; got != want {
			t.Errorf("%s %s light = %#x, want %#x", record.Name, record.StateJSON, got, want)
		}
	}
	if changed != 16 {
		t.Errorf("changed = %d, want 16", changed)
	}
	changed, err = applyRetailLightCorrections(records, properties, retail)
	if err != nil || changed != 0 {
		t.Errorf("second correction: changed=%d err=%v, want unchanged", changed, err)
	}
}

// 1.26.50.26 selects emission by trial-spawner state, independent of ominous.
func TestTrialSpawnerEmissionIsStateResolved(t *testing.T) {
	for ominous := 0; ominous < 2; ominous++ {
		for state, want := range []byte{0, 4, 8, 8, 8, 0} {
			record := Record{Name: "minecraft:trial_spawner", StateJSON: []byte(fmt.Sprintf(`{"ominous":{"type":"byte","value":%d},"trial_spawner_state":{"type":"int","value":%d}}`, ominous, state))}
			properties := []byte{0xf0}
			if _, err := applyRetailLightCorrections([]Record{record}, properties, nil); err != nil {
				t.Fatal(err)
			}
			if properties[0] != 0xf0|want {
				t.Fatalf("ominous=%d state=%d emission=%d want=%d", ominous, state, properties[0]&15, want)
			}
		}
	}
}

// Current accessors and the pinned pack schema define these state vectors.
func TestStateEmissionVectors(t *testing.T) {
	cases := []struct {
		name, key, state string
		want             byte
	}{
		{"vault", "vault_state", `"inactive"`, 6},
		{"vault", "vault_state", `"active"`, 12},
		{"vault", "vault_state", `"unlocking"`, 12},
		{"vault", "vault_state", `"ejecting"`, 12},
		{"respawn_anchor", "respawn_anchor_charge", "0", 0},
		{"respawn_anchor", "respawn_anchor_charge", "1", 3},
		{"respawn_anchor", "respawn_anchor_charge", "2", 7},
		{"respawn_anchor", "respawn_anchor_charge", "3", 11},
		{"respawn_anchor", "respawn_anchor_charge", "4", 15},
		{"sculk_sensor", "sculk_sensor_phase", "0", 0},
		{"sculk_sensor", "sculk_sensor_phase", "1", 1},
		{"sculk_sensor", "sculk_sensor_phase", "2", 0},
		{"calibrated_sculk_sensor", "sculk_sensor_phase", "1", 1},
	}
	for _, c := range cases {
		t.Run(c.name+c.state, func(t *testing.T) {
			record := Record{Name: "minecraft:" + c.name, StateJSON: []byte(fmt.Sprintf(`{"%s":{"value":%s}}`, c.key, c.state))}
			got, ok, err := stateEmission(record)
			if err != nil || !ok || got != c.want {
				t.Fatalf("emission=%d resolved=%v err=%v want=%d", got, ok, err, c.want)
			}
		})
	}
}
