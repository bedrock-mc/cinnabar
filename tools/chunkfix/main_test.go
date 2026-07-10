package main

import (
	"bytes"
	"encoding/json"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"sort"
	"testing"
)

func TestGenerateIsDeterministicAndPinned(t *testing.T) {
	if err := verifySourcePin(); err != nil {
		t.Fatalf("verify Dragonfly dependency pin: %v", err)
	}
	firstDir, secondDir := t.TempDir(), t.TempDir()
	if err := generate(firstDir); err != nil {
		t.Fatalf("generate first corpus: %v", err)
	}
	if err := generate(secondDir); err != nil {
		t.Fatalf("generate second corpus: %v", err)
	}

	firstManifestBytes := readFile(t, filepath.Join(firstDir, "manifest.json"))
	secondManifestBytes := readFile(t, filepath.Join(secondDir, "manifest.json"))
	if !bytes.Equal(firstManifestBytes, secondManifestBytes) {
		t.Fatal("manifest differs between identical generator runs")
	}
	if len(firstManifestBytes) == 0 || firstManifestBytes[len(firstManifestBytes)-1] != '\n' {
		t.Fatal("manifest must end in a newline")
	}

	var manifest fixtureManifest
	if err := json.Unmarshal(firstManifestBytes, &manifest); err != nil {
		t.Fatalf("decode manifest: %v", err)
	}
	if manifest.Source != (sourceDescriptor{Module: sourceModule, Version: sourceVersion, Commit: sourceCommit}) {
		t.Fatalf("source provenance = %+v", manifest.Source)
	}

	wantFiles := []string{
		"uniform_non_air.bin",
		"checkerboard.bin",
		"vertical_layers.bin",
		"two_storage_layers.bin",
		"bits_1.bin",
		"bits_2.bin",
		"bits_3.bin",
		"bits_4.bin",
		"bits_5.bin",
		"bits_6.bin",
		"bits_8.bin",
		"bits_16.bin",
	}
	if len(manifest.Fixtures) != len(wantFiles) {
		t.Fatalf("fixture count = %d, want %d", len(manifest.Fixtures), len(wantFiles))
	}
	for i, fixture := range manifest.Fixtures {
		if fixture.File != wantFiles[i] {
			t.Fatalf("fixture %d file = %q, want %q", i, fixture.File, wantFiles[i])
		}
		if fixture.Version != 9 || fixture.YIndex != -4 {
			t.Fatalf("%s header metadata = version %d/y %d", fixture.File, fixture.Version, fixture.YIndex)
		}
		first := readFile(t, filepath.Join(firstDir, fixture.File))
		second := readFile(t, filepath.Join(secondDir, fixture.File))
		if !bytes.Equal(first, second) {
			t.Fatalf("%s differs between identical generator runs", fixture.File)
		}
		version, yIndex, storages, err := inspectSubChunk(first)
		if err != nil {
			t.Fatalf("inspect %s: %v", fixture.File, err)
		}
		if version != fixture.Version || yIndex != fixture.YIndex || !slices.Equal(storages, fixture.Storages) {
			t.Fatalf(
				"%s binary metadata = version %d/y %d/%+v, manifest = %d/%d/%+v",
				fixture.File, version, yIndex, storages, fixture.Version, fixture.YIndex, fixture.Storages,
			)
		}
	}

	_, sourceFile, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("resolve test source path")
	}
	committedDir := filepath.Clean(filepath.Join(filepath.Dir(sourceFile), "..", "..", "crates", "world", "fixtures"))
	assertDirectoriesEqual(t, firstDir, committedDir)
}

func TestWidthFixturesUseMinimalPaletteBoundaries(t *testing.T) {
	want := []storageDescriptor{
		{BitsPerIndex: 1, PaletteLen: 2},
		{BitsPerIndex: 2, PaletteLen: 3},
		{BitsPerIndex: 3, PaletteLen: 5},
		{BitsPerIndex: 4, PaletteLen: 9},
		{BitsPerIndex: 5, PaletteLen: 17},
		{BitsPerIndex: 6, PaletteLen: 33},
		{BitsPerIndex: 8, PaletteLen: 65},
		{BitsPerIndex: 16, PaletteLen: 257},
	}
	specs := fixtureSpecs()
	if len(specs) != 4+len(want) {
		t.Fatalf("spec count = %d, want %d", len(specs), 4+len(want))
	}
	for i, descriptor := range want {
		got := specs[4+i].storages
		if len(got) != 1 || got[0] != descriptor {
			t.Fatalf("width spec %d = %+v, want %+v", i, got, descriptor)
		}
	}
}

func readFile(t *testing.T, path string) []byte {
	t.Helper()
	b, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read %s: %v", path, err)
	}
	return b
}

func assertDirectoriesEqual(t *testing.T, generatedDir, committedDir string) {
	t.Helper()
	generated := directoryFiles(t, generatedDir)
	committed := directoryFiles(t, committedDir)
	if !slices.Equal(generated, committed) {
		t.Fatalf("fixture file set drift: generated %v, committed %v", generated, committed)
	}
	for _, name := range generated {
		if !bytes.Equal(readFile(t, filepath.Join(generatedDir, name)), readFile(t, filepath.Join(committedDir, name))) {
			t.Fatalf("committed fixture %s differs from freshly generated output", name)
		}
	}
}

func directoryFiles(t *testing.T, dir string) []string {
	t.Helper()
	entries, err := os.ReadDir(dir)
	if err != nil {
		t.Fatalf("read fixture directory %s: %v", dir, err)
	}
	files := make([]string, 0, len(entries))
	for _, entry := range entries {
		if entry.IsDir() {
			t.Fatalf("unexpected directory in fixture corpus: %s", entry.Name())
		}
		files = append(files, entry.Name())
	}
	sort.Strings(files)
	return files
}
