package main

import (
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"strings"
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
	MissingNumericBiomeProjection       MissingProjection = "numeric_biome_projection"
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
	Dragonfly dragonflyFoundationSource `json:"dragonfly"`
	BDS       bdsFoundationSource       `json:"bds"`
}

type dragonflyFoundationSource struct {
	Commit string `json:"commit"`
	Blob   string `json:"blob"`
	SHA256 string `json:"sha256"`
	Size   uint64 `json:"size"`
}

type bdsFoundationSource struct {
	ArchiveSHA256    string `json:"archive_sha256"`
	ExecutableSHA256 string `json:"executable_sha256"`
	OverlaySHA256    string `json:"overlay_sha256"`
}

type projectionBindings struct {
	Block projectionBinding `json:"block"`
	Biome projectionBinding `json:"biome"`
	Light projectionBinding `json:"light"`
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
	if foundation.GameVersion != "1.26.40" || foundation.Protocol != 2168 {
		return errors.New("registry foundation must target game 1.26.40 and protocol 2168")
	}
	if foundation.Formats != (foundationFormats{Block: "BREG1003", Light: "LREG1001", Biome: "BIOREG01"}) {
		return errors.New("registry foundation format labels do not match the stable formats")
	}
	wantOutputs := foundationOutputs{
		Block: "crates/assets/data/block-registry-v2168.bin",
		Light: "crates/assets/data/block-light-registry-v2168.bin",
		Biome: "crates/assets/data/biome-registry-v2168.bin",
	}
	if foundation.Outputs != wantOutputs {
		return errors.New("registry foundation outputs must use the exact v2168 filenames")
	}
	if err := validateFoundationSources(foundation.Sources); err != nil {
		return err
	}
	switch foundation.Status {
	case FoundationBlocked:
		wantMissing := []MissingProjection{
			MissingRetailBlockProjection,
			MissingNumericBiomeProjection,
			MissingAuthoritativeLightProjection,
		}
		if !equalMissing(foundation.Missing, wantMissing) {
			return errors.New("blocked registry foundation must name exactly the three missing projections")
		}
		if foundation.ProjectionBindings != nil {
			return errors.New("blocked registry foundation must not claim projection bindings")
		}
	case FoundationReady:
		if len(foundation.Missing) != 0 {
			return errors.New("ready registry foundation must not retain missing projections")
		}
		if foundation.ProjectionBindings == nil {
			return errors.New("ready registry foundation requires three projection bindings")
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
	dragonfly := sources.Dragonfly
	if dragonfly.Commit != "0c2c404540fc651873c24a020b0a48778bd56295" ||
		dragonfly.Blob != "7006d9d46217425aab8e7d998f70c370b6b9c4eb" ||
		dragonfly.SHA256 != "1dc6d7ea26b48b5b5e4702762e463b95e59eb109f26c0c3b74115d12cb1941a7" ||
		dragonfly.Size != 2436125 {
		return errors.New("registry foundation Dragonfly source does not match the audited public identity")
	}
	if !validLowerHex(dragonfly.Commit, 20) || !validLowerHex(dragonfly.Blob, 20) || !validLowerHex(dragonfly.SHA256, 32) {
		return errors.New("registry foundation Dragonfly hashes must be lowercase hexadecimal")
	}
	bds := sources.BDS
	if bds.ArchiveSHA256 != "7b649671e1d88f8bd1499c580910f099e27533efc213f9faf5a5c68dd41a77c9" ||
		bds.ExecutableSHA256 != "e7775e636b9fdcbc354823d92d0c22c12738a2141d12557d856744293d258372" ||
		bds.OverlaySHA256 != "c52bbdfa8c92679595b5e342bee556a891a8aab91d5173f8670ff15e47e3efbb" {
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
