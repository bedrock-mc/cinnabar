package extension

import (
	"archive/zip"
	"bytes"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
)

// zipEntry is one file of a test .cxb.
type zipEntry struct {
	name string
	data []byte
}

// writeCXB writes a .cxb holding entries at path and returns its bytes.
func writeCXB(t *testing.T, path string, entries ...zipEntry) []byte {
	t.Helper()
	var buf bytes.Buffer
	w := zip.NewWriter(&buf)
	for _, e := range entries {
		f, err := w.CreateHeader(&zip.FileHeader{Name: e.name, Method: zip.Store})
		if err != nil {
			t.Fatal(err)
		}
		if _, err := f.Write(e.data); err != nil {
			t.Fatal(err)
		}
	}
	if err := w.Close(); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, buf.Bytes(), 0o644); err != nil {
		t.Fatal(err)
	}
	return buf.Bytes()
}

// component is a stand-in component file; the server half never reads it.
var component = zipEntry{"component.wasm", []byte("\x00asm\x01\x00\x00\x00")}

// resignedManifest is the golden manifest changed by change and signed with the fixture
// publisher seed, as a manifest.signed.json entry.
func resignedManifest(t *testing.T, change func(*Manifest)) zipEntry {
	t.Helper()
	_, _, publisher := testKeys(t)
	var manifest Manifest
	decodeGolden(t, "manifest_payload.json", &manifest)
	change(&manifest)
	document, err := Sign(ManifestDomain, manifest, publisher)
	if err != nil {
		t.Fatal(err)
	}
	data, err := Encode(document)
	if err != nil {
		t.Fatal(err)
	}
	return zipEntry{ManifestPath, data}
}

// renamed gives the manifest another package id, moving its channels into that namespace.
func renamed(id string) func(*Manifest) {
	return func(m *Manifest) {
		for i := range m.Channels {
			m.Channels[i].ID = id + strings.TrimPrefix(m.Channels[i].ID, m.ID)
		}
		m.ID = id
	}
}

// A bundle is offered by its whole-file digest and size, under the manifest it signs.
func TestReadBundleTakesItsSignedManifest(t *testing.T) {
	path := filepath.Join(t.TempDir(), "benergistics.cxb")
	data := writeCXB(t, path, zipEntry{ManifestPath, fixture(t, "manifest_signed.json")}, component)
	bundle, err := ReadBundle(path)
	if err != nil {
		t.Fatal(err)
	}
	if got, err := Encode(bundle.Manifest); err != nil || string(got) != string(fixture(t, "manifest_payload.json")) {
		t.Fatalf("manifest %s, %v; want the golden payload", got, err)
	}
	if bundle.Digest != Digest(data) || bundle.Bytes != uint64(len(data)) {
		t.Fatalf("digest %s and size %d, want %s and %d", bundle.Digest, bundle.Bytes, Digest(data), len(data))
	}
}

// A bundle whose manifest the client would refuse, or that the server could not route, fails
// startup instead of being offered.
func TestReadBundleRejects(t *testing.T) {
	var tampered SignedDocument
	decodeGolden(t, "manifest_signed.json", &tampered)
	flipped := map[bool]string{true: "1", false: "0"}[tampered.Signature[0] == '0']
	tampered.Signature = flipped + tampered.Signature[1:]
	tamperedData, err := Encode(tampered)
	if err != nil {
		t.Fatal(err)
	}
	for _, c := range []struct {
		name    string
		entries []zipEntry
		raw     []byte
	}{
		{name: "no manifest", entries: []zipEntry{component}},
		{name: "a tampered signature", entries: []zipEntry{{ManifestPath, tamperedData}, component}},
		{name: "another api", entries: []zipEntry{resignedManifest(t, func(m *Manifest) { m.API++ }), component}},
		{name: "another version", entries: []zipEntry{resignedManifest(t, func(m *Manifest) { m.Version++ }), component}},
		{name: "an id that is no identifier", entries: []zipEntry{resignedManifest(t, renamed("Bad")), component}},
		{name: "a channel outside its namespace", entries: []zipEntry{resignedManifest(t, func(m *Manifest) {
			m.Channels[0].ID = "other" + strings.TrimPrefix(m.Channels[0].ID, m.ID)
		}), component}},
		{name: "a channel schema declared twice", entries: []zipEntry{resignedManifest(t, func(m *Manifest) {
			m.Channels = append(m.Channels, m.Channels[0])
		}), component}},
		{name: "not a zip", raw: []byte("not a zip")},
		{name: "empty", raw: []byte{}},
	} {
		t.Run(c.name, func(t *testing.T) {
			path := filepath.Join(t.TempDir(), "b.cxb")
			if c.raw != nil {
				if err := os.WriteFile(path, c.raw, 0o644); err != nil {
					t.Fatal(err)
				}
			} else {
				writeCXB(t, path, c.entries...)
			}
			if bundle, err := ReadBundle(path); err == nil {
				t.Fatalf("read %+v", bundle.Manifest)
			}
		})
	}
}

// Every .cxb in the directory is offered, in byte order of the file names; other files are not
// bundles.
func TestReadBundlesTakesEveryBundleInNameOrder(t *testing.T) {
	dir := t.TempDir()
	writeCXB(t, filepath.Join(dir, "b.cxb"), zipEntry{ManifestPath, fixture(t, "manifest_signed.json")}, component)
	writeCXB(t, filepath.Join(dir, "a.cxb"), resignedManifest(t, renamed("another")), component)
	if err := os.WriteFile(filepath.Join(dir, "notes.txt"), []byte("not a bundle"), 0o644); err != nil {
		t.Fatal(err)
	}
	bundles, err := ReadBundles(dir)
	if err != nil {
		t.Fatal(err)
	}
	var ids []string
	for _, b := range bundles {
		ids = append(ids, b.Manifest.ID)
	}
	var golden Manifest
	decodeGolden(t, "manifest_payload.json", &golden)
	if want := []string{"another", golden.ID}; !slices.Equal(ids, want) {
		t.Fatalf("bundles %q, want %q", ids, want)
	}
	if _, err := ReadBundles(t.TempDir()); err == nil {
		t.Fatal("a directory without bundles was accepted")
	}
}

// TestReadBundleBoundsFileBeforeReading refuses an oversized sparse file without allocating its size.
func TestReadBundleBoundsFileBeforeReading(t *testing.T) {
	path := filepath.Join(t.TempDir(), "oversized.cxb")
	file, err := os.Create(path)
	if err != nil {
		t.Fatal(err)
	}
	if err := file.Truncate(int64(MaxBundleBytes) * 16); err != nil {
		file.Close()
		t.Fatal(err)
	}
	if err := file.Close(); err != nil {
		t.Fatal(err)
	}
	if _, err := ReadBundle(path); err == nil || !strings.Contains(err.Error(), "regular bundle file") {
		t.Fatalf("oversized file was not rejected before reading: %v", err)
	}
}

// TestReadBundlesStopsAtCountLimit refuses excess bundles before attempting to parse them.
func TestReadBundlesStopsAtCountLimit(t *testing.T) {
	dir := t.TempDir()
	for i := range MaxBundles {
		writeCXB(t, filepath.Join(dir, string(rune('a'+i))+bundleSuffix),
			zipEntry{ManifestPath, fixture(t, "manifest_signed.json")}, component)
	}
	if err := os.WriteFile(filepath.Join(dir, "z"+bundleSuffix), []byte("not a zip"), 0o644); err != nil {
		t.Fatal(err)
	}
	if _, err := ReadBundles(dir); err == nil || !strings.Contains(err.Error(), "more than") {
		t.Fatalf("count limit did not stop parsing: %v", err)
	}
}
