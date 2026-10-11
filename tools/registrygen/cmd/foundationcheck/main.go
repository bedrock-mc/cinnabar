package main

import (
	"encoding/hex"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"strings"

	shared "github.com/bedrock-mc/protocolgen/generated/data"
	"github.com/hashimthearab/rust-mcbe/tools/registrygen/internal/targetpin"
)

const maxManifestBytes = 16 * 1024

type manifest struct {
	Schema             string              `json:"schema"`
	Status             string              `json:"status"`
	GameVersion        string              `json:"game_version"`
	Protocol           uint32              `json:"protocol"`
	Formats            formats             `json:"formats"`
	Outputs            outputs             `json:"outputs"`
	Sources            sources             `json:"sources"`
	Missing            []string            `json:"missing,omitempty"`
	ProjectionBindings *projectionBindings `json:"projection_bindings,omitempty"`
}

type formats struct {
	Block string `json:"block"`
	Light string `json:"light"`
	Biome string `json:"biome"`
}

type outputs struct {
	Block string `json:"block"`
	Light string `json:"light"`
	Biome string `json:"biome"`
}

type sources struct {
	Protocolgen sharedFoundationSource `json:"protocolgen"`
	BDS         bdsSource              `json:"bds"`
}

type sharedFoundationSource struct {
	Module           string `json:"module"`
	SourceLockSHA256 string `json:"source_lock_sha256"`
	CloudburstRef    string `json:"cloudburst_ref"`
}

type bdsSource struct {
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

func main() {
	os.Exit(run(os.Args[1:], os.Stdout, os.Stderr))
}

func run(arguments []string, stdout, stderr io.Writer) int {
	flags := flag.NewFlagSet("foundationcheck", flag.ContinueOnError)
	flags.SetOutput(stderr)
	manifestPath := flags.String("manifest", "", "registry foundation manifest to validate")
	expectBlocked := flags.Bool("expect-blocked", false, "succeed only for a valid blocked foundation")
	if err := flags.Parse(arguments); err != nil {
		return 1
	}
	if flags.NArg() != 0 {
		fmt.Fprintln(stderr, "registry-foundation: positional arguments are not accepted")
		return 1
	}
	status, missing, err := validateFile(*manifestPath)
	if err != nil {
		fmt.Fprintln(stderr, "registry-foundation:", err)
		return 1
	}
	if status == "blocked" {
		fmt.Fprintf(stdout, "registry-foundation: status=blocked missing=%s\n", strings.Join(missing, ","))
		if *expectBlocked {
			return 0
		}
		return 2
	}
	if *expectBlocked {
		fmt.Fprintln(stderr, "registry-foundation: expected blocked status, got ready")
		return 1
	}
	fmt.Fprintln(stdout, "registry-foundation: status=ready")
	return 0
}

func validateFile(path string) (string, []string, error) {
	if path == "" {
		return "", nil, errors.New("-manifest is required")
	}
	file, err := os.Open(path)
	if err != nil {
		return "", nil, fmt.Errorf("open manifest: %w", err)
	}
	defer file.Close()
	payload, err := io.ReadAll(io.LimitReader(file, maxManifestBytes+1))
	if err != nil {
		return "", nil, fmt.Errorf("read manifest: %w", err)
	}
	if len(payload) > maxManifestBytes {
		return "", nil, fmt.Errorf("manifest exceeds %d bytes", maxManifestBytes)
	}
	decoder := json.NewDecoder(strings.NewReader(string(payload)))
	decoder.DisallowUnknownFields()
	var value manifest
	if err := decoder.Decode(&value); err != nil {
		return "", nil, fmt.Errorf("decode manifest: %w", err)
	}
	var trailing any
	if err := decoder.Decode(&trailing); !errors.Is(err, io.EOF) {
		if err == nil {
			return "", nil, errors.New("manifest has trailing JSON")
		}
		return "", nil, fmt.Errorf("decode trailing manifest data: %w", err)
	}
	if err := validate(value); err != nil {
		return "", nil, err
	}
	return value.Status, append([]string(nil), value.Missing...), nil
}

func validate(value manifest) error {
	if value.Schema != "cinnabar.registry-foundation.v1" {
		return errors.New("unexpected schema")
	}
	if value.GameVersion != "1.26.50" || value.Protocol != 2193 {
		return errors.New("foundation must target game 1.26.50 and protocol 2193")
	}
	if value.Formats != (formats{Block: "BREG1003", Light: "LREG1001", Biome: "BIOREG01"}) {
		return errors.New("unexpected stable format labels")
	}
	wantOutputs := outputs{
		Block: "crates/assets/data/block-registry-v2193.bin",
		Light: "crates/assets/data/block-light-registry-v2193.bin",
		Biome: "crates/assets/data/biome-registry-v2193.bin",
	}
	if value.Outputs != wantOutputs {
		return errors.New("outputs must use exact v2193 filenames")
	}
	if err := validateSources(value.Sources); err != nil {
		return err
	}
	wantMissing := []string{
		"retail_block_projection",
		"authoritative_light_projection",
	}
	switch value.Status {
	case "blocked":
		if strings.Join(value.Missing, "\x00") != strings.Join(wantMissing, "\x00") {
			return errors.New("blocked foundation must name exactly the block and light projections")
		}
		if value.ProjectionBindings == nil || value.ProjectionBindings.Block != nil ||
			value.ProjectionBindings.Light != nil || value.ProjectionBindings.Biome == nil ||
			value.ProjectionBindings.Biome.SHA256 != "e3ba3d96a66fa49b3b7d94ae6b67b4cc5d8961789c91275080d3922909b25c2a" {
			return errors.New("blocked foundation must bind only the exact biome projection")
		}
	case "ready":
		if len(value.Missing) != 0 || value.ProjectionBindings == nil || value.ProjectionBindings.Block == nil ||
			value.ProjectionBindings.Biome == nil || value.ProjectionBindings.Light == nil {
			return errors.New("ready foundation requires three projection bindings and no missing entries")
		}
		if value.ProjectionBindings.Biome.SHA256 != "e3ba3d96a66fa49b3b7d94ae6b67b4cc5d8961789c91275080d3922909b25c2a" {
			return errors.New("ready foundation must preserve the exact biome projection binding")
		}
		blockHash, err := targetpin.BlockHash()
		if err != nil {
			return err
		}
		lightHash, err := targetpin.LightHash()
		if err != nil {
			return err
		}
		if value.ProjectionBindings.Block.SHA256 != blockHash ||
			value.ProjectionBindings.Light.SHA256 != lightHash {
			return errors.New("ready foundation must bind the exact block and light projections")
		}
		for _, digest := range []string{
			value.ProjectionBindings.Block.SHA256,
			value.ProjectionBindings.Biome.SHA256,
			value.ProjectionBindings.Light.SHA256,
		} {
			if !lowerHex(digest, 32) {
				return errors.New("ready projection hashes must be lowercase SHA-256")
			}
		}
	default:
		return errors.New("status must be blocked or ready")
	}
	return nil
}

func validateSources(value sources) error {
	catalog := value.Protocolgen
	if catalog.Module != "github.com/bedrock-mc/protocolgen/generated/data" || catalog.SourceLockSHA256 != shared.SourceLockSHA256 || catalog.CloudburstRef != shared.CloudburstRef {
		return errors.New("registry foundation shared catalog identity does not match the pinned module")
	}
	bds := value.BDS
	if bds.ArchiveSHA256 != "2c9b98d07d2504786996f2335980e88bd969b4a77514925e75471a1349995825" ||
		bds.ExecutableSHA256 != "19c88569af2e4b7d984e999055a31cbcb0799dacf8bbbf7371eda42f5772a443" ||
		bds.OverlaySHA256 != "f7cc20dd63cc799381368b55104d8a7b7dd20fc654a655a2188833db9f663339" {
		return errors.New("unexpected BDS source identities")
	}
	for _, digest := range []string{bds.ArchiveSHA256, bds.ExecutableSHA256, bds.OverlaySHA256} {
		if !lowerHex(digest, 32) {
			return errors.New("BDS hashes must be lowercase SHA-256")
		}
	}
	return nil
}

func lowerHex(value string, decodedBytes int) bool {
	if len(value) != decodedBytes*2 || value != strings.ToLower(value) {
		return false
	}
	decoded, err := hex.DecodeString(value)
	return err == nil && len(decoded) == decodedBytes
}
