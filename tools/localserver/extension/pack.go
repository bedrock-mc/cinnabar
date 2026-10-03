package extension

import (
	"encoding/json"
	"errors"
	"fmt"
	"io/fs"
	"math"
	"os"
	"path/filepath"
	"strconv"
	"strings"

	"github.com/google/uuid"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/resource"
)

// MarkerPackDir is the directory of the marker pack in the resource pack directory that Dragonfly
// loads. Dragonfly offers its packs as optional unless told otherwise, and so this one.
const MarkerPackDir = "cinnabar-extension-offer"

// RevisionFile is the file in the world directory that holds the last offered revision.
const RevisionFile = "extension-revision"

// WriteMarkerPack replaces the marker pack in resources with one that holds the marker at
// MarkerPath. The pack's UUID follows the server key and its version is the offer's revision, so a
// client's pack cache, which keys on both, never serves an older offer.
func (s *Server) WriteMarkerPack(resources string) error {
	if s.offer.Revision > math.MaxInt32 {
		return fmt.Errorf("revision %d does not fit a pack version", s.offer.Revision)
	}
	engine, err := packVersion(protocol.CurrentVersion)
	if err != nil {
		return err
	}
	version := resource.Version{1, 0, int(s.offer.Revision)}
	manifest, err := json.Marshal(resource.Manifest{
		FormatVersion: 2,
		Header: resource.Header{
			Name:               "Cinnabar client part offer",
			Description:        "The signed offer of this server's optional client parts",
			UUID:               uuid.NewSHA1(uuid.NameSpaceURL, []byte(Carrier+"/marker/"+s.offer.ServerKey)),
			Version:            version,
			MinimumGameVersion: engine,
		},
		Modules: []resource.Module{{
			UUID:        uuid.NewSHA1(uuid.NameSpaceURL, []byte(Carrier+"/marker/"+s.offer.ServerKey+"/resources")).String(),
			Description: "The offer marker",
			Type:        "resources",
			Version:     version,
		}},
	})
	if err != nil {
		return err
	}
	marker, err := Encode(s.marker)
	if err != nil {
		return err
	}
	dir := filepath.Join(resources, MarkerPackDir)
	if err := os.RemoveAll(dir); err != nil {
		return err
	}
	if err := os.MkdirAll(filepath.Join(dir, filepath.Dir(filepath.FromSlash(MarkerPath))), 0o755); err != nil {
		return err
	}
	if err := os.WriteFile(filepath.Join(dir, "manifest.json"), manifest, 0o644); err != nil {
		return err
	}
	return os.WriteFile(filepath.Join(dir, filepath.FromSlash(MarkerPath)), marker, 0o644)
}

// RemoveMarkerPack removes the marker pack that an earlier start wrote into resources, so a server
// without the server half advertises nothing.
func RemoveMarkerPack(resources string) error {
	return os.RemoveAll(filepath.Join(resources, MarkerPackDir))
}

// packVersion parses a Bedrock version a.b.c.
func packVersion(text string) (resource.Version, error) {
	var v resource.Version
	parts := strings.Split(text, ".")
	if len(parts) != len(v) {
		return v, fmt.Errorf("game version %q is not a.b.c", text)
	}
	for i, part := range parts {
		n, err := strconv.Atoi(part)
		if err != nil {
			return v, fmt.Errorf("game version %q: %w", text, err)
		}
		v[i] = n
	}
	return v, nil
}

// NextRevision stores and returns the revision after the one in path, 1 when there is none. The
// client rejects an offer below a revision it has seen, so a file that holds no revision is an
// error rather than a reason to start again from 1.
func NextRevision(path string) (uint64, error) {
	var last uint64
	data, err := os.ReadFile(path)
	switch {
	case errors.Is(err, fs.ErrNotExist):
	case err != nil:
		return 0, err
	default:
		if last, err = strconv.ParseUint(strings.TrimSpace(string(data)), 10, 64); err != nil {
			return 0, fmt.Errorf("%s does not hold a revision: %w", path, err)
		}
		if last == math.MaxUint64 {
			return 0, fmt.Errorf("%s holds the last revision", path)
		}
	}
	next := last + 1
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		return 0, err
	}
	temp := path + ".tmp"
	if err := os.WriteFile(temp, []byte(strconv.FormatUint(next, 10)+"\n"), 0o644); err != nil {
		return 0, err
	}
	if err := os.Rename(temp, path); err != nil {
		return 0, err
	}
	return next, nil
}
