package main

import (
	"errors"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
)

func TestDefinitionRangesFollowStableCanonicalOrdering(t *testing.T) {
	entries := []sourceEntry{
		{name: "minecraft:only", hash: 8},
		{name: "minecraft:hardcoded", hash: 3},
		{name: "minecraft:another", hash: 6},
		{name: "minecraft:only", hash: 8},
	}
	table, err := buildTable(entries, map[string]bool{
		"minecraft:only": true, "minecraft:another": true,
	}, "binding")
	if err != nil {
		t.Fatal(err)
	}
	if !strings.HasSuffix(string(table), "minecraft:another\t1\t1\nminecraft:only\t2\t2\n") {
		t.Fatalf("unexpected ranges:\n%s", table)
	}
	if entries[0].name != "minecraft:only" {
		t.Fatal("generation changed the source order")
	}
}

func TestMissingAndNoncontiguousDefinitionStatesFail(t *testing.T) {
	_, err := buildTable([]sourceEntry{}, map[string]bool{"minecraft:missing": true}, "binding")
	if err == nil || !strings.Contains(err.Error(), "missing from the projection") {
		t.Fatalf("missing definition error = %v", err)
	}
	entries := []sourceEntry{
		{name: "minecraft:only", hash: 1},
		{name: "minecraft:other", hash: 2},
		{name: "minecraft:only", hash: 3},
	}
	_, err = buildTable(entries, map[string]bool{"minecraft:only": true}, "binding")
	if err == nil || !strings.Contains(err.Error(), "noncontiguous") {
		t.Fatalf("noncontiguous definition error = %v", err)
	}
}

func TestEqualHashesUseNameOrderAndPreservePermutationOrder(t *testing.T) {
	type permutation struct {
		entry sourceEntry
		state int
	}
	states := []permutation{
		{entry: sourceEntry{name: "minecraft:z", hash: 1}, state: 30},
		{entry: sourceEntry{name: "minecraft:a", hash: 1}, state: 20},
		{entry: sourceEntry{name: "minecraft:z", hash: 1}, state: 31},
		{entry: sourceEntry{name: "minecraft:a", hash: 1}, state: 21},
	}
	entries := make([]sourceEntry, 0, len(states))
	for _, state := range states {
		entries = append(entries, state.entry)
	}
	table, err := buildTable(entries, map[string]bool{"minecraft:a": true, "minecraft:z": true}, "binding")
	if err != nil {
		t.Fatal(err)
	}
	if !strings.HasSuffix(string(table), "minecraft:a\t0\t2\nminecraft:z\t2\t2\n") {
		t.Fatalf("equal-hash ranges do not use lexical name order:\n%s", table)
	}
	slices.SortStableFunc(states, func(a, b permutation) int {
		return compareEntries(a.entry, b.entry)
	})
	got := make([]int, 0, len(states))
	for _, state := range states {
		got = append(got, state.state)
	}
	if !slices.Equal(got, []int{20, 21, 30, 31}) {
		t.Fatalf("state permutation order = %v", got)
	}
}

func TestChangedPinnedInputFails(t *testing.T) {
	path := filepath.Join(t.TempDir(), "source")
	if err := os.WriteFile(path, []byte("changed"), 0o644); err != nil {
		t.Fatal(err)
	}
	if _, err := readPinned(path, strings.Repeat("0", 64)); err == nil {
		t.Fatal("changed source passed its registry binding")
	}
}

func TestPinnedMetadataReproduces(t *testing.T) {
	err := run(options{root: "../../../..", check: true})
	if errors.Is(err, os.ErrNotExist) {
		missingInput := strings.Contains(err.Error(), "behavior_pack") ||
			strings.Contains(err.Error(), "block_states.nbt")
		if missingInput {
			t.Skipf("missing fixture: pinned palette generation input: %v", err)
		}
	}
	if err != nil {
		t.Fatal(err)
	}
}
