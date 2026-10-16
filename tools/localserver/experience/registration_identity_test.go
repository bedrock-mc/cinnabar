package experience

import (
	"slices"
	"strings"
	"testing"
)

// Restarted helpers cannot describe items differently from the finalized server registry.
func TestRegistrationRefusesChangedItems(t *testing.T) {
	registered := fakeLoaded()
	registered.Items = []ItemDef{
		{ID: "fake:cell", DisplayName: "Cell", Icon: "/fake/cell.png", MaxStack: 1},
		{ID: "fake:other", DisplayName: "Other", Icon: "/fake/other.png", MaxStack: 16},
	}
	cases := []struct {
		name   string
		change func(*Loaded)
	}{
		{"id", func(loaded *Loaded) { loaded.Items[0].ID = "fake:replacement" }},
		{"name", func(loaded *Loaded) { loaded.Items[0].DisplayName = "Replacement" }},
		{"icon", func(loaded *Loaded) { loaded.Items[0].Icon = "/fake/replacement.png" }},
		{"stack", func(loaded *Loaded) { loaded.Items[0].MaxStack = 64 }},
		{"added", func(loaded *Loaded) { loaded.Items = append(loaded.Items, ItemDef{ID: "fake:new", MaxStack: 1}) }},
		{"removed", func(loaded *Loaded) { loaded.Items = loaded.Items[:1] }},
		{"order", func(loaded *Loaded) { slices.Reverse(loaded.Items) }},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			loaded := registered
			loaded.Items = slices.Clone(registered.Items)
			tc.change(&loaded)
			if err := matchRegistration(registered, loaded); err == nil || !strings.Contains(err.Error(), "item") {
				t.Fatalf("changed registration: %v; want an item mismatch", err)
			}
		})
	}
	loaded := registered
	loaded.Version += "-fixed"
	loaded.Items = slices.Clone(registered.Items)
	if err := matchRegistration(registered, loaded); err != nil {
		t.Fatalf("unchanged items with new helper version: %v", err)
	}
	for _, empty := range [][]ItemDef{nil, {}} {
		loaded := fakeLoaded()
		loaded.Items = empty
		if err := matchRegistration(fakeLoaded(), loaded); err != nil {
			t.Fatalf("equivalent empty item list: %v", err)
		}
	}
}

// A rejected item change follows the existing helper shutdown and quarantine path.
func TestReloadRefusesNewItemRegistration(t *testing.T) {
	log, _ := testLog(t)
	s, _ := startFake(t, "ok", log, startOptions{})
	loaded := fakeLoaded()
	loaded.Items = []ItemDef{{ID: "fake:new", DisplayName: "New", Icon: "/fake/new.png", MaxStack: 1}}
	rescript(t, s, "ok", loaded)
	if err := s.Reload(); err == nil || !strings.Contains(err.Error(), "item") {
		t.Fatalf("Reload: %v; want an item mismatch", err)
	}
	if !s.Quarantined() || currentHelper(s) != nil {
		t.Fatal("a rejected item replacement must leave no active helper")
	}
}
