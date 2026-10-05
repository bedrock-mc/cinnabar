package localworld

import (
	"archive/zip"
	"bytes"
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"testing"
	"time"
)

// incompressibleZip keeps the archive large enough that half of it is a meaningful resume point.
func incompressibleZip(t *testing.T) []byte {
	t.Helper()
	binary := make([]byte, 256<<10)
	_, _ = rand.Read(binary)
	return buildZip(t, map[string]string{"bedrock_server": string(binary), "server.properties": "x"})
}

func readyProvisioner(t *testing.T, f *fakeMojang) *Provisioner {
	t.Helper()
	p := testProvisioner(t, f)
	if err := p.AcceptEULA(); err != nil {
		t.Fatal(err)
	}
	return p
}

func lastRange(f *fakeMojang) string {
	got := "none"
	for {
		select {
		case r := <-f.ranges:
			got = r
		default:
			return got
		}
	}
}

func installedSHA(t *testing.T, bin string) string {
	t.Helper()
	raw, err := os.ReadFile(filepath.Join(filepath.Dir(bin), "manifest.json"))
	if err != nil {
		t.Fatal(err)
	}
	i := bytes.Index(raw, []byte(`"zip_sha256": "`))
	return string(raw[i+15 : i+15+64])
}

func sha(b []byte) string { s := sha256.Sum256(b); return hex.EncodeToString(s[:]) }

// An interrupted download continues from its .zip.part with a Range request instead of starting over.
func TestDownloadResumesPartialArchive(t *testing.T) {
	archive := incompressibleZip(t)
	f := newFakeMojang(t, "1.26.52.3", archive)
	p := readyProvisioner(t, f)
	half := len(archive) / 2
	part := filepath.Join(p.downloadsDir(), "bedrock-server-1.26.52.3"+partSuffix)
	_ = os.MkdirAll(filepath.Dir(part), 0o700)
	if err := os.WriteFile(part, archive[:half], 0o600); err != nil {
		t.Fatal(err)
	}
	bin, err := p.Ensure(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if got := lastRange(f); got != fmt.Sprintf("bytes=%d-", half) {
		t.Fatalf("Range = %q", got)
	}
	if got := installedSHA(t, bin); got != sha(archive) {
		t.Fatalf("manifest digest %s, want the whole archive's %s", got, sha(archive))
	}
	if _, err := os.Stat(part); !os.IsNotExist(err) {
		t.Fatalf("part survived install: %v", err)
	}
}

// A resumed prefix that is not the archive fails extraction once, then the next attempt starts clean.
func TestCorruptPartIsDiscardedAfterFailedUnpack(t *testing.T) {
	archive := incompressibleZip(t)
	f := newFakeMojang(t, "1.26.52.3", archive)
	p := readyProvisioner(t, f)
	part := filepath.Join(p.downloadsDir(), "bedrock-server-1.26.52.3"+partSuffix)
	_ = os.MkdirAll(filepath.Dir(part), 0o700)
	_ = os.WriteFile(part, bytes.Repeat([]byte{0xAB}, len(archive)/2), 0o600)
	if _, err := p.Ensure(context.Background()); err == nil {
		t.Fatal("a corrupt resumed archive installed")
	}
	if _, err := os.Stat(part); !os.IsNotExist(err) {
		t.Fatalf("corrupt part kept: %v", err)
	}
	if _, err := p.Ensure(context.Background()); err != nil {
		t.Fatal(err)
	}
	if got := lastRange(f); got != "" {
		t.Fatalf("retry sent Range %q, want a full download", got)
	}
}

// A server that goes silent mid-download fails within the stall timeout, and the bytes so far are resumed.
func TestStalledDownloadTimesOutThenResumes(t *testing.T) {
	archive := incompressibleZip(t)
	f := newFakeMojang(t, "1.26.52.3", archive)
	p := readyProvisioner(t, f)
	p.stallTimeout = 200 * time.Millisecond
	half := int64(len(archive) / 2)
	f.stallAfter.Store(half)
	began := time.Now()
	_, err := p.Ensure(context.Background())
	if err == nil || !strings.Contains(err.Error(), errDownloadStalled.Error()) {
		t.Fatalf("err = %v", err)
	}
	if took := time.Since(began); took > 5*time.Second {
		t.Fatalf("stall detected after %s", took)
	}
	part := filepath.Join(p.downloadsDir(), "bedrock-server-1.26.52.3"+partSuffix)
	if info, err := os.Stat(part); err != nil || info.Size() != half {
		t.Fatalf("part after stall = %v, %v; want %d bytes", info, err, half)
	}
	f.stallAfter.Store(0)
	if _, err := p.Ensure(context.Background()); err != nil {
		t.Fatal(err)
	}
	if got := lastRange(f); got != "bytes="+strconv.FormatInt(half, 10)+"-" {
		t.Fatalf("Range = %q", got)
	}
}

func zipReader(t *testing.T, archive []byte) *zip.Reader {
	t.Helper()
	r, err := zip.NewReader(bytes.NewReader(archive), int64(len(archive)))
	if err != nil {
		t.Fatal(err)
	}
	return r
}

// Parallel workers must write every file into nested folders with exact contents.
func TestParallelExtractionWritesEveryFile(t *testing.T) {
	files := map[string]string{}
	for i := range 400 {
		files[fmt.Sprintf("packs/p%d/sub%d/file%d.json", i%7, i%13, i)] = strings.Repeat(strconv.Itoa(i), i%50+1)
	}
	files["bedrock_server"] = "elf"
	root := t.TempDir()
	if err := extractArchive(root, zipReader(t, buildZip(t, files)).File, bdsLimits); err != nil {
		t.Fatal(err)
	}
	for name, body := range files {
		raw, err := os.ReadFile(filepath.Join(root, filepath.FromSlash(name)))
		if err != nil || string(raw) != body {
			t.Fatalf("%s = %q, %v", name, raw, err)
		}
	}
}

// Limits apply to the whole listing before anything is written.
func TestExtractionRejectsListingsOverLimits(t *testing.T) {
	small := unpackLimits{maxEntries: 3, maxFileBytes: 1000, maxTotalBytes: 1500, minRatioSample: 8, maxEntryRatio: 20, maxArchiveRatio: 10}
	random := make([]byte, 1001)
	_, _ = rand.Read(random)
	for name, files := range map[string]map[string]string{
		"entries":   {"a": "1", "b": "2", "c": "3", "d": "4"},
		"file size": {"a": string(random)},
		"total":     {"a": string(random[:900]), "b": string(random[100:])},
		"ratio":     {"a": strings.Repeat("z", 1000)},
		"duplicate": {"Dir/File": "1", "dir/file": "2"},
		"slip":      {"../evil": "x"},
	} {
		root := filepath.Join(t.TempDir(), "root")
		_ = os.MkdirAll(root, 0o700)
		if err := extractArchive(root, zipReader(t, buildZip(t, files)).File, small); err == nil {
			t.Fatalf("%s: accepted", name)
		}
		if entries, _ := os.ReadDir(root); len(entries) != 0 {
			t.Fatalf("%s: wrote %v before rejecting", name, entries)
		}
	}
}

// A new install removes older versions and stale downloads, but never world data linked into them.
func TestInstallPrunesOldVersionsAndKeepsWorldData(t *testing.T) {
	f := newFakeMojang(t, "1.26.52.3", buildZip(t, map[string]string{"bedrock_server": "bin"}))
	p := readyProvisioner(t, f)
	world := filepath.Join(t.TempDir(), "world")
	_ = os.MkdirAll(world, 0o700)
	_ = os.WriteFile(filepath.Join(world, "level.dat"), []byte("keep"), 0o600)
	old := filepath.Join(p.Root, "1.26.40.1")
	if err := linkWorld(filepath.Join(old, "worlds", "linked"), world); err != nil {
		t.Fatal(err)
	}
	_ = os.MkdirAll(filepath.Join(old, "worlds", "mount-point"), 0o700)
	_ = os.WriteFile(filepath.Join(old, "bedrock_server"), []byte("old"), 0o700)
	occupied := filepath.Join(p.Root, "1.26.30.0", "worlds", "real")
	_ = os.MkdirAll(occupied, 0o700)
	_ = os.WriteFile(filepath.Join(occupied, "level.dat"), []byte("x"), 0o600)
	_ = os.MkdirAll(filepath.Join(p.Root, "1.26.41.0.partial"), 0o700)
	_ = os.MkdirAll(filepath.Join(p.Root, "not-a-version"), 0o700)
	_ = os.MkdirAll(p.downloadsDir(), 0o700)
	stale := filepath.Join(p.downloadsDir(), "bedrock-server-1.26.40.1"+partSuffix)
	_ = os.WriteFile(stale, []byte("old"), 0o600)

	if _, err := p.Ensure(context.Background()); err != nil {
		t.Fatal(err)
	}
	for path, want := range map[string]bool{
		old: false, filepath.Join(p.Root, "1.26.41.0.partial"): false, stale: false,
		occupied: true, filepath.Join(p.Root, "not-a-version"): true, p.eulaPath(): true,
		filepath.Join(p.Root, "1.26.52.3", "bedrock_server"): true,
	} {
		if _, err := os.Stat(path); (err == nil) != want {
			t.Fatalf("%s exists = %v, want %v", path, err == nil, want)
		}
	}
	if raw, err := os.ReadFile(filepath.Join(world, "level.dat")); err != nil || string(raw) != "keep" {
		t.Fatalf("world data = %q, %v", raw, err)
	}
}
