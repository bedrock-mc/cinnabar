package main

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
)

func TestCrossFamilyRoutesCoverEveryMatchingPaletteState(t *testing.T) {
	root := filepath.Join("..", "..")
	data, err := os.ReadFile(filepath.Join(root, "crates", "assets", "data", "block-family-routing.json"))
	if err != nil {
		t.Fatal(err)
	}
	var catalog struct {
		CrossedPlanes []string `json:"crossed_planes"`
	}
	if err := json.Unmarshal(data, &catalog); err != nil {
		t.Fatal(err)
	}
	wanted := make(map[string]bool)
	for _, name := range catalog.CrossedPlanes {
		wanted[name] = false
	}
	palette, err := v2193SourceRecords()
	if err != nil {
		t.Fatal(err)
	}
	carrier, err := os.ReadFile(filepath.Join(root, "crates", "assets", "data", "block-registry-v2193.bin"))
	if err != nil {
		t.Fatal(err)
	}
	_, records, err := decodeBREGRecords(carrier, v2193BlockProtocol)
	if err != nil {
		t.Fatal(err)
	}
	for _, state := range palette {
		if _, routed := wanted[state.Name]; !routed {
			continue
		}
		wanted[state.Name] = true
		record := records[state.SequentialID]
		if record.Name != state.Name || record.ModelFamily != ModelFamilyCross {
			t.Errorf("state %d %s has name/family %s/%d, want its own crossed-plane route", state.SequentialID, state.Name, record.Name, record.ModelFamily)
		}
	}
	for name, present := range wanted {
		if !present {
			t.Errorf("routing entry %s has no state in the independent palette", name)
		}
	}
}
