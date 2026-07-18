package main

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
)

func TestRunInstallsAndReverifiesExactLocalBundle(t *testing.T) {
	root := t.TempDir()
	first := writeFixture(t, root, "protocol_info.json", []byte(`{"version":{"major":1,"minor":26,"patch":30,"protocol_version":1001}}`))
	second := writeFixture(t, root, "blocks.json", []byte("blocks"))
	manifest := writeManifest(t, root, []sourceFixture{
		{id: "pmmp-bedrock-data", destination: "pmmp", files: []string{first}},
		{id: "prismarinejs-minecraft-data", destination: "prismarine", files: []string{second}},
	})
	destination := filepath.Join(root, "installed")

	if err := run(manifest, destination, nil); err != nil {
		t.Fatalf("first acquisition failed: %v", err)
	}
	if err := run(manifest, destination, nil); err != nil {
		t.Fatalf("verified acquisition failed: %v", err)
	}
	assertBytes(t, filepath.Join(destination, "pmmp", "protocol_info.json"), mustRead(t, first))
	assertBytes(t, filepath.Join(destination, "prismarine", "blocks.json"), mustRead(t, second))
}

func TestRunRejectsTamperingAndTraversalWithoutPublishing(t *testing.T) {
	root := t.TempDir()
	fixture := writeFixture(t, root, "source.bin", []byte("expected"))
	manifest := writeManifest(t, root, []sourceFixture{{
		id: "fixture", destination: "safe", files: []string{fixture},
	}})
	var document map[string]any
	decodeJSON(t, manifest, &document)
	source := document["sources"].([]any)[0].(map[string]any)
	file := source["files"].([]any)[0].(map[string]any)
	file["install_path"] = "../escape.bin"
	writeJSON(t, manifest, document)
	destination := filepath.Join(root, "installed")

	if err := run(manifest, destination, nil); err == nil {
		t.Fatal("traversal manifest unexpectedly succeeded")
	}
	if _, err := os.Stat(destination); !os.IsNotExist(err) {
		t.Fatalf("invalid manifest published destination: %v", err)
	}
}

func TestRunRejectsChangedSourceBytesAndUnexpectedInstalledEntries(t *testing.T) {
	root := t.TempDir()
	fixture := writeFixture(t, root, "source.bin", []byte("expected"))
	manifest := writeManifest(t, root, []sourceFixture{{
		id: "fixture", destination: "safe", files: []string{fixture},
	}})
	destination := filepath.Join(root, "installed")
	if err := os.WriteFile(fixture, []byte("tampered"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := run(manifest, destination, nil); err == nil {
		t.Fatal("changed source bytes unexpectedly succeeded")
	}
	if _, err := os.Stat(destination); !os.IsNotExist(err) {
		t.Fatalf("failed acquisition published destination: %v", err)
	}
	if err := os.WriteFile(fixture, []byte("expected"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := run(manifest, destination, nil); err != nil {
		t.Fatalf("valid acquisition failed: %v", err)
	}
	if err := os.WriteFile(filepath.Join(destination, "unexpected"), []byte("x"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := run(manifest, destination, nil); err == nil {
		t.Fatal("unexpected installed entry was accepted")
	}
}

func TestRunRejectsRedirectedDownloadCacheWithoutWritingThroughIt(t *testing.T) {
	root := t.TempDir()
	fixture := writeFixture(t, root, "source.bin", []byte("expected"))
	manifest := writeManifest(t, root, []sourceFixture{{
		id: "fixture", destination: "safe", files: []string{fixture},
	}})
	destination := filepath.Join(root, "installed")
	redirect := filepath.Join(root, "redirect")
	if err := os.Mkdir(redirect, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(redirect, destination+".downloads"); err != nil {
		t.Skipf("directory symlinks are unavailable: %v", err)
	}

	if err := run(manifest, destination, nil); err == nil {
		t.Fatal("redirected download cache unexpectedly succeeded")
	}
	entries, err := os.ReadDir(redirect)
	if err != nil {
		t.Fatal(err)
	}
	if len(entries) != 0 {
		t.Fatalf("redirect target was modified: %v", entries)
	}
}

type sourceFixture struct {
	id          string
	destination string
	files       []string
}

func writeManifest(t *testing.T, root string, fixtures []sourceFixture) string {
	t.Helper()
	sources := make([]any, 0, len(fixtures))
	for _, fixture := range fixtures {
		files := make([]any, 0, len(fixture.files))
		for _, path := range fixture.files {
			bytes := mustRead(t, path)
			digest := sha256.Sum256(bytes)
			files = append(files, map[string]any{
				"upstream_path": filepath.Base(path),
				"install_path":  filepath.Base(path),
				"url":           fileURL(path),
				"sha256":        hex.EncodeToString(digest[:]),
				"size_bytes":    len(bytes),
			})
		}
		sources = append(sources, map[string]any{
			"id": fixture.id, "repository": "https://example.invalid", "commit": "0123456789012345678901234567890123456789",
			"destination": fixture.destination, "license": map[string]any{"spdx": "MIT"}, "files": files,
		})
	}
	manifest := filepath.Join(root, "sources.json")
	writeJSON(t, manifest, map[string]any{
		"schema":          1,
		"protocol":        map[string]any{"game_version": "1.26.30", "protocol_version": 1001},
		"artifact_policy": "local-only",
		"limits": map[string]any{
			"max_sources": 16, "max_files_per_source": 64, "max_file_bytes": 1024,
			"max_total_bytes": 4096, "download_buffer_bytes": 4096, "request_timeout_seconds": 30,
		},
		"sources": sources,
	})
	return manifest
}

func writeFixture(t *testing.T, root, name string, bytes []byte) string {
	t.Helper()
	path := filepath.Join(root, name)
	if err := os.WriteFile(path, bytes, 0o600); err != nil {
		t.Fatal(err)
	}
	return path
}

func fileURL(path string) string { return "file:///" + filepath.ToSlash(path) }
func mustRead(t *testing.T, path string) []byte {
	t.Helper()
	bytes, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	return bytes
}
func assertBytes(t *testing.T, path string, expected []byte) {
	t.Helper()
	actual := mustRead(t, path)
	if string(actual) != string(expected) {
		t.Fatalf("%s mismatch", path)
	}
}
func writeJSON(t *testing.T, path string, value any) {
	t.Helper()
	bytes, err := json.Marshal(value)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, bytes, 0o600); err != nil {
		t.Fatal(err)
	}
}
func decodeJSON(t *testing.T, path string, value any) {
	t.Helper()
	if err := json.Unmarshal(mustRead(t, path), value); err != nil {
		t.Fatal(err)
	}
}
