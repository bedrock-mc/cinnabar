package main

import (
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"strings"

	shared "github.com/bedrock-mc/protocolgen/generated/data"
	"github.com/hashimthearab/rust-mcbe/tools/registrygen/internal/targetpin"
)

const maxFoundationBytes = 16 * 1024

type FoundationStatus string

const (
	FoundationBlocked FoundationStatus = "blocked"
	FoundationReady   FoundationStatus = "ready"
)

type MissingProjection string

const (
	MissingRetailBlockProjection        MissingProjection = "retail_block_projection"
	MissingAuthoritativeLightProjection MissingProjection = "authoritative_light_projection"
)

type FoundationCheckResult struct {
	Status  FoundationStatus
	Missing []MissingProjection
}

func (result FoundationCheckResult) MissingStrings() []string {
	values := make([]string, len(result.Missing))
	for index, value := range result.Missing {
		values[index] = string(value)
	}
	return values
}

type registryFoundation struct {
	Schema             string              `json:"schema"`
	Status             FoundationStatus    `json:"status"`
	GameVersion        string              `json:"game_version"`
	Protocol           uint32              `json:"protocol"`
	Formats            foundationFormats   `json:"formats"`
	Outputs            foundationOutputs   `json:"outputs"`
	Sources            foundationSources   `json:"sources"`
	Missing            []MissingProjection `json:"missing,omitempty"`
	ProjectionBindings *projectionBindings `json:"projection_bindings,omitempty"`
}

type foundationFormats struct {
	Block string `json:"block"`
	Light string `json:"light"`
	Biome string `json:"biome"`
}

type foundationOutputs struct {
	Block string `json:"block"`
	Light string `json:"light"`
	Biome string `json:"biome"`
}

type foundationSources struct {
	Protocolgen sharedFoundationSource `json:"protocolgen"`
	BDS         bdsFoundationSource    `json:"bds"`
}

type sharedFoundationSource struct {
	Module           string `json:"module"`
	SourceLockSHA256 string `json:"source_lock_sha256"`
	CloudburstRef    string `json:"cloudburst_ref"`
}

type bdsFoundationSource struct {
	ArchiveSHA256    string `json:"archive_sha256"`
	ExecutableSHA256 string `json:"executable_sha256"`
	OverlaySHA256    string `json:"overlay_sha256"`
}

type projectionBindings struct {
	Block *projectionBinding `json:"block,omitempty"`
	Biome *projectionBinding `json:"biome,omitempty"`
	Light *projectionBinding `json:"light,omitempty"`
}

type projectionBinding struct {
	SHA256 string `json:"sha256"`
}

func ValidateRegistryFoundation(reader io.Reader) (FoundationCheckResult, error) {
	limited := io.LimitReader(reader, maxFoundationBytes+1)
	payload, err := io.ReadAll(limited)
	if err != nil {
		return FoundationCheckResult{}, fmt.Errorf("read registry foundation: %w", err)
	}
	if len(payload) > maxFoundationBytes {
		return FoundationCheckResult{}, fmt.Errorf("registry foundation exceeds %d bytes", maxFoundationBytes)
	}

	decoder := json.NewDecoder(strings.NewReader(string(payload)))
	decoder.DisallowUnknownFields()
	var foundation registryFoundation
	if err := decoder.Decode(&foundation); err != nil {
		return FoundationCheckResult{}, fmt.Errorf("decode registry foundation: %w", err)
	}
	var trailing any
	if err := decoder.Decode(&trailing); !errors.Is(err, io.EOF) {
		if err == nil {
			return FoundationCheckResult{}, errors.New("registry foundation has trailing JSON")
		}
		return FoundationCheckResult{}, fmt.Errorf("decode trailing registry foundation data: %w", err)
	}
	if err := validateFoundationFields(foundation); err != nil {
		return FoundationCheckResult{}, err
	}
	return FoundationCheckResult{
		Status:  foundation.Status,
		Missing: append([]MissingProjection(nil), foundation.Missing...),
	}, nil
}

func validateFoundationFields(foundation registryFoundation) error {
	if foundation.Schema != "cinnabar.registry-foundation.v1" {
		return errors.New("registry foundation schema must be cinnabar.registry-foundation.v1")
	}
	if foundation.GameVersion != "1.26.50" || foundation.Protocol != 2193 {
		return errors.New("registry foundation must target game 1.26.50 and protocol 2193")
	}
	if foundation.Formats != (foundationFormats{Block: "BREG1003", Light: "LREG1001", Biome: "BIOREG01"}) {
		return errors.New("registry foundation format labels do not match the stable formats")
	}
	wantOutputs := foundationOutputs{
		Block: "crates/assets/data/block-registry-v2193.bin",
		Light: "crates/assets/data/block-light-registry-v2193.bin",
		Biome: "crates/assets/data/biome-registry-v2193.bin",
	}
	if foundation.Outputs != wantOutputs {
		return errors.New("registry foundation outputs must use the exact v2193 filenames")
	}
	if err := validateFoundationSources(foundation.Sources); err != nil {
		return err
	}
	switch foundation.Status {
	case FoundationBlocked:
		wantMissing := []MissingProjection{
			MissingRetailBlockProjection,
			MissingAuthoritativeLightProjection,
		}
		if !equalMissing(foundation.Missing, wantMissing) {
			return errors.New("blocked registry foundation must name exactly the block and light projections")
		}
		if foundation.ProjectionBindings == nil || foundation.ProjectionBindings.Block != nil ||
			foundation.ProjectionBindings.Light != nil || foundation.ProjectionBindings.Biome == nil ||
			foundation.ProjectionBindings.Biome.SHA256 != "e3ba3d96a66fa49b3b7d94ae6b67b4cc5d8961789c91275080d3922909b25c2a" {
			return errors.New("blocked registry foundation must bind only the exact biome projection")
		}
	case FoundationReady:
		if len(foundation.Missing) != 0 {
			return errors.New("ready registry foundation must not retain missing projections")
		}
		if foundation.ProjectionBindings == nil || foundation.ProjectionBindings.Block == nil ||
			foundation.ProjectionBindings.Biome == nil || foundation.ProjectionBindings.Light == nil {
			return errors.New("ready registry foundation requires three projection bindings")
		}
		if foundation.ProjectionBindings.Biome.SHA256 != "e3ba3d96a66fa49b3b7d94ae6b67b4cc5d8961789c91275080d3922909b25c2a" {
			return errors.New("ready registry foundation must preserve the exact biome projection binding")
		}
		blockHash, err := targetpin.BlockHash()
		if err != nil {
			return err
		}
		lightHash, err := targetpin.LightHash()
		if err != nil {
			return err
		}
		if foundation.ProjectionBindings.Block.SHA256 != blockHash ||
			foundation.ProjectionBindings.Light.SHA256 != lightHash {
			return errors.New("ready registry foundation must bind the exact block and light projections")
		}
		for label, digest := range map[string]string{
			"block": foundation.ProjectionBindings.Block.SHA256,
			"biome": foundation.ProjectionBindings.Biome.SHA256,
			"light": foundation.ProjectionBindings.Light.SHA256,
		} {
			if !validLowerHex(digest, 32) {
				return fmt.Errorf("ready %s projection SHA-256 must be lowercase hexadecimal", label)
			}
		}
	default:
		return errors.New("registry foundation status must be blocked or ready")
	}
	return nil
}

func validateFoundationSources(sources foundationSources) error {
	catalog := sources.Protocolgen
	if catalog.Module != "github.com/bedrock-mc/protocolgen/generated/data" || catalog.SourceLockSHA256 != shared.SourceLockSHA256 || catalog.CloudburstRef != shared.CloudburstRef {
		return errors.New("registry foundation shared catalog identity does not match the pinned module")
	}
	bds := sources.BDS
	if bds.ArchiveSHA256 != "2c9b98d07d2504786996f2335980e88bd969b4a77514925e75471a1349995825" ||
		bds.ExecutableSHA256 != "19c88569af2e4b7d984e999055a31cbcb0799dacf8bbbf7371eda42f5772a443" ||
		bds.OverlaySHA256 != "f7cc20dd63cc799381368b55104d8a7b7dd20fc654a655a2188833db9f663339" {
		return errors.New("registry foundation BDS source does not match the audited public identities")
	}
	for _, digest := range []string{bds.ArchiveSHA256, bds.ExecutableSHA256, bds.OverlaySHA256} {
		if !validLowerHex(digest, 32) {
			return errors.New("registry foundation BDS hashes must be lowercase hexadecimal")
		}
	}
	return nil
}

func validLowerHex(value string, decodedBytes int) bool {
	if value != strings.ToLower(value) || len(value) != decodedBytes*2 {
		return false
	}
	decoded, err := hex.DecodeString(value)
	return err == nil && len(decoded) == decodedBytes
}

func equalMissing(left, right []MissingProjection) bool {
	if len(left) != len(right) {
		return false
	}
	for index := range left {
		if left[index] != right[index] {
			return false
		}
	}
	return true
}
