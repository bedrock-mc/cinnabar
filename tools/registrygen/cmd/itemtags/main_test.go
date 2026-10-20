package main

import (
	"errors"
	"os"
	"reflect"
	"strings"
	"testing"

	shareditem "github.com/cinnabar-mc/protocolgen/generated/data/item"
)

// TestCompactSortsNativeMemberships checks inversion, stable ordering and borrowed data ownership.
func TestCompactSortsNativeMemberships(t *testing.T) {
	items := []shareditem.Runtime{{Name: "minecraft:z"}, {Name: "minecraft:x"}, {Name: "minecraft:y"}, {Name: "minecraft:untagged"}}
	tags := map[string][]string{
		"minecraft:z": {"minecraft:b", "minecraft:a"},
		"minecraft:x": {"minecraft:a"},
		"minecraft:y": {"minecraft:b"},
	}
	table, err := compact(items, func(name string) []string { return tags[name] })
	if err != nil {
		t.Fatal(err)
	}
	lines := strings.Split(strings.TrimSpace(table), "\n")
	if len(lines) != 3 || lines[1] != "minecraft:a\tminecraft:x minecraft:z" || lines[2] != "minecraft:b\tminecraft:y minecraft:z" {
		t.Fatalf("table = %q", table)
	}
	if items[0].Name != "minecraft:z" || !reflect.DeepEqual(tags["minecraft:z"], []string{"minecraft:b", "minecraft:a"}) {
		t.Fatal("compaction changed borrowed catalog data")
	}
}

// TestCompactRejectsInvalidMemberships rejects malformed identities and repeated edges.
func TestCompactRejectsInvalidMemberships(t *testing.T) {
	for _, scenario := range []struct {
		name  string
		items []shareditem.Runtime
		tags  []string
	}{
		{"empty registry", nil, nil},
		{"invalid item", []shareditem.Runtime{{Name: "Not An Id"}}, []string{"minecraft:a"}},
		{"duplicate item", []shareditem.Runtime{{Name: "minecraft:x"}, {Name: "minecraft:x"}}, nil},
		{"invalid tag", []shareditem.Runtime{{Name: "minecraft:x"}}, []string{"Not A Tag"}},
		{"duplicate membership", []shareditem.Runtime{{Name: "minecraft:x"}}, []string{"minecraft:a", "minecraft:a"}},
	} {
		t.Run(scenario.name, func(t *testing.T) {
			if _, err := compact(scenario.items, func(string) []string { return scenario.tags }); err == nil {
				t.Fatal("invalid native membership succeeded")
			}
		})
	}
}

// TestSharedCatalogReproducesCommittedTags checks all real memberships and their source identity.
func TestSharedCatalogReproducesCommittedTags(t *testing.T) {
	var output strings.Builder
	if err := run(&output); err != nil {
		t.Fatal(err)
	}
	expected, err := os.ReadFile("../../../../crates/protocol/data/item_tags_native.tsv")
	if err != nil {
		t.Fatal(err)
	}
	if output.String() != string(expected) {
		t.Fatal("native item-tag table is stale; regenerate it with go -C tools/registrygen run ./cmd/itemtags")
	}
}

// failedTagWriter exposes write failures without retaining generated data.
type failedTagWriter struct{ err error }

// Write returns the configured failure so callers cannot report a successful generation.
func (writer failedTagWriter) Write([]byte) (int, error) { return 0, writer.err }

// TestRunReportsWriteFailures keeps an unsuccessful output from appearing complete.
func TestRunReportsWriteFailures(t *testing.T) {
	want := errors.New("output unavailable")
	if err := run(failedTagWriter{want}); !errors.Is(err, want) {
		t.Fatalf("error = %v, want %v", err, want)
	}
}
