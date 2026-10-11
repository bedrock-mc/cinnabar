package main

import (
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

type familyRoutingCatalog struct {
	Schema        uint32   `json:"schema"`
	CrossedPlanes []string `json:"crossed_planes"`
}

// The generator starts in this repository; later test directory changes must not select another catalog.
var familyRouting, familyRoutingError = loadFamilyRouting()

// loadFamilyRouting finds the shared asset catalog above the generator's working directory.
func loadFamilyRouting() (familyRoutingCatalog, error) {
	dir, err := os.Getwd()
	if err != nil {
		return familyRoutingCatalog{}, err
	}
	for {
		data, err := os.ReadFile(filepath.Join(dir, "crates", "assets", "data", "block-family-routing.json"))
		if err == nil {
			return parseFamilyRouting(data)
		}
		if !os.IsNotExist(err) {
			return familyRoutingCatalog{}, err
		}
		parent := filepath.Dir(dir)
		if parent == dir {
			return familyRoutingCatalog{}, fmt.Errorf("block-family routing catalog not found")
		}
		dir = parent
	}
}

// parseFamilyRouting rejects unsupported catalogs and duplicate or unordered canonical names.
func parseFamilyRouting(data []byte) (familyRoutingCatalog, error) {
	var catalog familyRoutingCatalog
	decoder := json.NewDecoder(strings.NewReader(string(data)))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&catalog); err != nil {
		return catalog, err
	}
	if err := decoder.Decode(new(any)); err != io.EOF {
		return catalog, fmt.Errorf("trailing block-family routing data")
	}
	if catalog.Schema != 1 || len(catalog.CrossedPlanes) == 0 {
		return catalog, fmt.Errorf("invalid block-family routing catalog")
	}
	for index, name := range catalog.CrossedPlanes {
		if !strings.HasPrefix(name, "minecraft:") || len(name) > 256 || strings.ContainsAny(name, "\r\n\t") || (index > 0 && catalog.CrossedPlanes[index-1] >= name) {
			return catalog, fmt.Errorf("invalid or unordered block-family route %q", name)
		}
	}
	return catalog, nil
}

// applyFamilyRouting writes the rendering disposition without changing identity, collision or material facts.
func applyFamilyRouting(record *Record) error {
	if familyRoutingError != nil {
		return familyRoutingError
	}
	index := sort.SearchStrings(familyRouting.CrossedPlanes, record.Name)
	if index < len(familyRouting.CrossedPlanes) && familyRouting.CrossedPlanes[index] == record.Name {
		record.ModelFamily = ModelFamilyCross
	}
	return nil
}
