package extension

import (
	"archive/zip"
	"bytes"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"slices"
	"strings"
)

// Bundle is one .cxb that the server offers: the manifest it signs, and the digest and size by
// which the offer names the whole file.
type Bundle struct {
	Manifest Manifest
	Digest   string
	Bytes    uint64
}

// bundleSuffix marks the bundles of an -extension-cxb directory.
const bundleSuffix = ".cxb"

// ReadBundles reads every .cxb file in dir, in byte order of the names. A directory without one
// is an error.
func ReadBundles(dir string) ([]Bundle, error) {
	entries, err := os.ReadDir(dir) // sorted by name
	if err != nil {
		return nil, err
	}
	var bundles []Bundle
	for _, entry := range entries {
		if entry.IsDir() || !strings.HasSuffix(entry.Name(), bundleSuffix) {
			continue
		}
		if len(bundles) == MaxBundles {
			return nil, fmt.Errorf("more than %d bundles in %s", MaxBundles, dir)
		}
		bundle, err := ReadBundle(filepath.Join(dir, entry.Name()))
		if err != nil {
			return nil, err
		}
		bundles = append(bundles, bundle)
	}
	if len(bundles) == 0 {
		return nil, fmt.Errorf("no %s bundle in %s", bundleSuffix, dir)
	}
	return bundles, nil
}

// ReadBundle reads the .cxb at path: its manifest must be signed by the publisher key it names,
// at the client's API, and declare channels the server can route. The client verifies the rest
// of the bundle against the offer; `cinnabar-cxb build` has already checked it with the client's
// verifier.
func ReadBundle(path string) (Bundle, error) {
	bundle, err := readBundle(path)
	if err != nil {
		return Bundle{}, fmt.Errorf("bundle %s: %w", path, err)
	}
	return bundle, nil
}

// readBundle bounds the file before allocation and checks the signed manifest.
func readBundle(path string) (Bundle, error) {
	file, err := os.Open(path)
	if err != nil {
		return Bundle{}, err
	}
	defer file.Close()
	info, err := file.Stat()
	if err != nil {
		return Bundle{}, err
	}
	if !info.Mode().IsRegular() || info.Size() <= 0 || info.Size() > MaxBundleBytes {
		return Bundle{}, fmt.Errorf("not a regular bundle file of 1 to %d bytes", MaxBundleBytes)
	}
	// Bound the read as well, because a regular file can grow after Stat.
	data, err := io.ReadAll(io.LimitReader(file, MaxBundleBytes+1))
	if err != nil {
		return Bundle{}, err
	}
	if len(data) == 0 || len(data) > MaxBundleBytes {
		return Bundle{}, fmt.Errorf("%d bytes is not a bundle size", len(data))
	}
	archive, err := zip.NewReader(bytes.NewReader(data), int64(len(data)))
	if err != nil {
		return Bundle{}, err
	}
	signed, err := readManifestEntry(archive)
	if err != nil {
		return Bundle{}, err
	}
	var document SignedDocument
	if err := Decode(signed, &document); err != nil {
		return Bundle{}, fmt.Errorf("%s: %w", ManifestPath, err)
	}
	// The manifest names its publisher key; the offer will name the same key, so the client
	// checks the signature against exactly this one.
	payload, err := unhex(document.Payload)
	if err != nil {
		return Bundle{}, err
	}
	var manifest Manifest
	if err := Decode(payload, &manifest); err != nil {
		return Bundle{}, fmt.Errorf("%s: %w", ManifestPath, err)
	}
	if _, err := document.Verify(manifest.PublisherKey, ManifestDomain, MaxMarkerBytes/2, &manifest); err != nil {
		return Bundle{}, fmt.Errorf("%s: %w", ManifestPath, err)
	}
	if err := checkManifest(&manifest); err != nil {
		return Bundle{}, err
	}
	return Bundle{Manifest: manifest, Digest: Digest(data), Bytes: uint64(len(data))}, nil
}

// readManifestEntry reads the signed manifest of archive, at most MaxMarkerBytes as the client
// allows.
func readManifestEntry(archive *zip.Reader) ([]byte, error) {
	i := slices.IndexFunc(archive.File, func(f *zip.File) bool { return f.Name == ManifestPath })
	if i < 0 {
		return nil, fmt.Errorf("no %s", ManifestPath)
	}
	entry, err := archive.File[i].Open()
	if err != nil {
		return nil, err
	}
	defer entry.Close()
	signed, err := io.ReadAll(io.LimitReader(entry, MaxMarkerBytes+1))
	if err != nil {
		return nil, err
	}
	if len(signed) > MaxMarkerBytes {
		return nil, fmt.Errorf("%s over %d bytes", ManifestPath, MaxMarkerBytes)
	}
	return signed, nil
}

// checkManifest applies the client's manifest rules that the server relies on to route: the API,
// an identifier id, and channels in its namespace, each (id, schema) once.
func checkManifest(m *Manifest) error {
	if m.Version != WireVersion || m.API != APIVersion {
		return fmt.Errorf("manifest version %d api %d, want %d and %d", m.Version, m.API, WireVersion, APIVersion)
	}
	if !Identifier(m.ID) {
		return fmt.Errorf("package id %q is not an identifier", m.ID)
	}
	if len(m.Channels) > MaxChannels {
		return fmt.Errorf("%d channels, over %d", len(m.Channels), MaxChannels)
	}
	type key struct {
		id     string
		schema uint16
	}
	seen := make(map[key]bool, len(m.Channels))
	for _, c := range m.Channels {
		if !strings.HasPrefix(c.ID, m.ID+".") || !Identifier(c.ID) || len(c.Fields) > MaxChannelFields {
			return fmt.Errorf("channel %q is not a channel of %q", c.ID, m.ID)
		}
		if seen[key{c.ID, c.Schema}] {
			return fmt.Errorf("channel %q schema %d declared twice", c.ID, c.Schema)
		}
		seen[key{c.ID, c.Schema}] = true
	}
	return nil
}
