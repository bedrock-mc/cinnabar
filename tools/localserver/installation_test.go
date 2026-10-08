package main

import (
	"bytes"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/hashimthearab/rust-mcbe/tools/localserver/experience"
)

// TestInstalledDefinitionsSurviveRestart checks that updates retain every previously registered block.
func TestInstalledDefinitionsSurviveRestart(t *testing.T) {
	dir := t.TempDir()
	store, err := experience.OpenStore(dir)
	if err != nil {
		t.Fatal(err)
	}
	initial := []experience.Loaded{{ID: "probe", Blocks: []experience.BlockDef{{ID: "probe:first"}, {ID: "probe:second"}}}}
	if err := store.SetInstalled(initial); err != nil {
		t.Fatal(err)
	}
	store, err = experience.OpenStore(dir)
	if err != nil {
		t.Fatal(err)
	}
	manifest := filepath.Join(dir, "_installed.json")
	before, err := os.ReadFile(manifest)
	if err != nil {
		t.Fatal(err)
	}
	for _, blocks := range [][]experience.BlockDef{
		{{ID: "probe:first"}},
		{{ID: "probe:first"}, {ID: "probe:renamed"}},
	} {
		err := store.SetInstalled([]experience.Loaded{{ID: "probe", Blocks: blocks}})
		if err == nil || !strings.Contains(err.Error(), "probe:second") {
			t.Fatalf("removed block accepted: %v", err)
		}
	}
	after, err := os.ReadFile(manifest)
	if err != nil || !bytes.Equal(before, after) {
		t.Fatalf("rejected update changed installation manifest: %q, %v", after, err)
	}
	// Reordering, additions and presentation changes are compatible, and additions become required.
	updated := []experience.Loaded{{ID: "probe", Blocks: []experience.BlockDef{
		{ID: "probe:second", DisplayName: "New name"}, {ID: "probe:third"}, {ID: "probe:first"},
	}}}
	if err := store.SetInstalled(updated); err != nil {
		t.Fatal(err)
	}
	store, err = experience.OpenStore(dir)
	if err != nil {
		t.Fatal(err)
	}
	if err := store.ValidateInstalled(initial); err == nil || !strings.Contains(err.Error(), "probe:third") {
		t.Fatalf("added block not retained after restart: %v", err)
	}
}

// TestInstalledWorldRequiresFlag rejects an existing installation before opening the server.
func TestInstalledWorldRequiresFlag(t *testing.T) {
	dir := t.TempDir()
	store, err := experience.OpenStore(filepath.Join(dir, experienceDataDir))
	if err != nil {
		t.Fatal(err)
	}
	if err := store.SetInstalled([]experience.Loaded{{ID: "probe"}}); err != nil {
		t.Fatal(err)
	}
	var stdout bytes.Buffer
	err = run([]string{"-dir", dir, "-addr", "127.0.0.1:0"}, strings.NewReader("stop\n"), &stdout, io.Discard)
	if err == nil || !strings.Contains(err.Error(), "-experiences") {
		t.Fatalf("missing flag accepted: %v", err)
	}
	if stdout.Len() != 0 {
		t.Fatalf("server started: %q", stdout.String())
	}
	if _, err := os.Stat(filepath.Join(dir, "db")); !os.IsNotExist(err) {
		t.Fatalf("world opened before validation: %v", err)
	}
}

// TestLegacyInstallationNeedsExplicitMigration prevents inferring historical blocks from new code.
func TestLegacyInstallationNeedsExplicitMigration(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "_installed.json")
	legacy := []byte(`{"schema":1,"ids":["probe"]}`)
	if err := os.WriteFile(path, legacy, 0o644); err != nil {
		t.Fatal(err)
	}
	store, err := experience.OpenStore(dir)
	if err != nil {
		t.Fatal(err)
	}
	err = store.SetInstalled([]experience.Loaded{{ID: "probe", Blocks: []experience.BlockDef{{ID: "probe:renamed"}}}})
	if err == nil || !strings.Contains(err.Error(), "original installed block definitions") {
		t.Fatalf("unsafe migration accepted: %v", err)
	}
	got, err := os.ReadFile(path)
	if err != nil || !bytes.Equal(got, legacy) {
		t.Fatalf("legacy manifest changed: %q, %v", got, err)
	}
	if err := experience.CheckDisabled(dir); err == nil {
		t.Fatal("legacy installation accepted without flag")
	}
}
