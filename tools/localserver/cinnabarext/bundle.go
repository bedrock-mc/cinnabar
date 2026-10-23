package cinnabarext

import (
	"fmt"
	"io"
	"os"
	"path/filepath"
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

// ReadBundle validates the complete archive with cinnabar-cxb before returning digest-bound
// metadata. The client verifies the archive again at its own trust boundary.
func ReadBundle(path string) (Bundle, error) {
	bundle, err := readBundle(path)
	if err != nil {
		return Bundle{}, fmt.Errorf("bundle %s: %w", path, err)
	}
	return bundle, nil
}

// readBundle bounds one archive snapshot before passing it to the offline verifier.
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
	return verifyBundle(data)
}
