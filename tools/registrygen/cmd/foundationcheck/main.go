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
	Dragonfly dragonflySource `json:"dragonfly"`
	BDS       bdsSource       `json:"bds"`
}

type dragonflySource struct {
	Commit string `json:"commit"`
	Blob   string `json:"blob"`
	SHA256 string `json:"sha256"`
	Size   uint64 `json:"size"`
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
	if value.GameVersion != "1.26.40" || value.Protocol != 2168 {
		return errors.New("foundation must target game 1.26.40 and protocol 2168")
	}
	if value.Formats != (formats{Block: "BREG1003", Light: "LREG1001", Biome: "BIOREG01"}) {
		return errors.New("unexpected stable format labels")
	}
	wantOutputs := outputs{
		Block: "crates/assets/data/block-registry-v2168.bin",
		Light: "crates/assets/data/block-light-registry-v2168.bin",
		Biome: "crates/assets/data/biome-registry-v2168.bin",
	}
	if value.Outputs != wantOutputs {
		return errors.New("outputs must use exact v2168 filenames")
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
			value.ProjectionBindings.Biome.SHA256 != "5209a8ec6d9b2690d062c124e206dc0f565d1937601c181798dbffbd9904272c" {
			return errors.New("blocked foundation must bind only the exact biome projection")
		}
	case "ready":
		if len(value.Missing) != 0 || value.ProjectionBindings == nil || value.ProjectionBindings.Block == nil ||
			value.ProjectionBindings.Biome == nil || value.ProjectionBindings.Light == nil {
			return errors.New("ready foundation requires three projection bindings and no missing entries")
		}
		if value.ProjectionBindings.Biome.SHA256 != "5209a8ec6d9b2690d062c124e206dc0f565d1937601c181798dbffbd9904272c" {
			return errors.New("ready foundation must preserve the exact biome projection binding")
		}
		if value.ProjectionBindings.Block.SHA256 != "e3768f6d70195b22ac3843f6ef49261a80cd83284bc9741c7eb4a446def6bec8" ||
			value.ProjectionBindings.Light.SHA256 != "88bac8fd074e392930321d12f46b291f0557d89dd87392a13fb3b5025bfcd272" {
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
	dragonfly := value.Dragonfly
	if dragonfly.Commit != "0c2c404540fc651873c24a020b0a48778bd56295" ||
		dragonfly.Blob != "7006d9d46217425aab8e7d998f70c370b6b9c4eb" ||
		dragonfly.SHA256 != "1dc6d7ea26b48b5b5e4702762e463b95e59eb109f26c0c3b74115d12cb1941a7" ||
		dragonfly.Size != 2436125 {
		return errors.New("unexpected Dragonfly source identity")
	}
	if !lowerHex(dragonfly.Commit, 20) || !lowerHex(dragonfly.Blob, 20) || !lowerHex(dragonfly.SHA256, 32) {
		return errors.New("Dragonfly hashes must be lowercase hexadecimal")
	}
	bds := value.BDS
	if bds.ArchiveSHA256 != "7b649671e1d88f8bd1499c580910f099e27533efc213f9faf5a5c68dd41a77c9" ||
		bds.ExecutableSHA256 != "e7775e636b9fdcbc354823d92d0c22c12738a2141d12557d856744293d258372" ||
		bds.OverlaySHA256 != "c52bbdfa8c92679595b5e342bee556a891a8aab91d5173f8670ff15e47e3efbb" {
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
