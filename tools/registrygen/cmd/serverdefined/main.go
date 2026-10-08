// Command serverdefined binds vanilla behavior definitions to canonical palette ranges.
package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"hash/fnv"
	"os"
	"os/exec"
	"path/filepath"
	"slices"
	"strings"

	"github.com/sandertv/gophertunnel/minecraft/nbt"
)

type options struct {
	root   string
	states string
	output string
	check  bool
}

type targetManifest struct {
	GameVersion string `json:"game_version"`
	Protocol    int    `json:"wire_protocol"`
	Hashes      struct {
		BlockRegistry string `json:"block_registry"`
	} `json:"hashes"`
	Artifacts struct {
		BlockRegistry string `json:"block_registry"`
	} `json:"artifacts"`
}

type projectionManifest struct {
	GameVersion string `json:"game_version"`
	Protocol    int    `json:"protocol"`
	Source      struct {
		Module  string `json:"module"`
		Version string `json:"version"`
		SHA256  string `json:"sha256"`
		Size    int    `json:"size"`
	} `json:"source"`
	Projection struct {
		States int `json:"states"`
	} `json:"projection"`
	Output struct {
		Path   string `json:"path"`
		SHA256 string `json:"sha256"`
	} `json:"output"`
}

type sourceEntry struct {
	name string
	hash uint64
}

func main() {
	var opts options
	flag.StringVar(&opts.root, "root", "../..", "repository root")
	flag.StringVar(&opts.states, "states", "", "pinned block_states.nbt (default: Go module cache)")
	flag.StringVar(&opts.output, "out", "", "output TSV (default: active assets metadata)")
	flag.BoolVar(&opts.check, "check", false, "compare the generated table without writing")
	flag.Parse()
	if err := run(opts); err != nil {
		fmt.Fprintln(os.Stderr, "serverdefined:", err)
		os.Exit(1)
	}
}

func run(opts options) error {
	var target targetManifest
	if err := readJSON(filepath.Join(opts.root, "assets/bedrock-target.json"), &target); err != nil {
		return err
	}
	var projection projectionManifest
	projectionPath := filepath.Join(opts.root, fmt.Sprintf("assets/block-projection-v%d.json", target.Protocol))
	if err := readJSON(projectionPath, &projection); err != nil {
		return err
	}
	versionMatches := projection.Protocol == target.Protocol && projection.GameVersion == target.GameVersion
	registryMatches := projection.Output.Path == target.Artifacts.BlockRegistry &&
		projection.Output.SHA256 == target.Hashes.BlockRegistry
	if !versionMatches || !registryMatches {
		return errors.New("projection binding does not match the active target")
	}
	if _, err := readPinned(filepath.Join(opts.root, projection.Output.Path), projection.Output.SHA256); err != nil {
		return err
	}
	var vanilla struct {
		CacheDir string `json:"cache_dir"`
	}
	if err := readJSON(filepath.Join(opts.root, "assets/vanilla-source.json"), &vanilla); err != nil {
		return err
	}
	names, err := definitionNames(filepath.Join(opts.root, vanilla.CacheDir, "behavior_pack/blocks"))
	if err != nil {
		return err
	}
	if opts.states == "" {
		cache, err := exec.Command("go", "env", "GOMODCACHE").Output()
		if err != nil {
			return fmt.Errorf("locate Go module cache: %w", err)
		}
		opts.states = filepath.Join(strings.TrimSpace(string(cache)),
			projection.Source.Module+"@"+projection.Source.Version, "server/world/block_states.nbt")
	}
	data, err := readPinned(opts.states, projection.Source.SHA256)
	if err != nil {
		return err
	}
	if len(data) != projection.Source.Size {
		return errors.New("block-state source size does not match the projection")
	}
	entries, err := decodeStates(data, projection.Projection.States)
	if err != nil {
		return err
	}
	table, err := buildTable(entries, names, projection.Output.SHA256)
	if err != nil {
		return err
	}
	if opts.output == "" {
		opts.output = filepath.Join(opts.root,
			fmt.Sprintf("crates/assets/data/server-defined-blocks-v%d.tsv", target.Protocol))
	}
	if opts.check {
		current, err := os.ReadFile(opts.output)
		if err != nil {
			return err
		}
		if !bytes.Equal(current, table) {
			return fmt.Errorf("%s differs from the pinned generated table", opts.output)
		}
		return nil
	}
	return os.WriteFile(opts.output, table, 0o644)
}

func readJSON(path string, value any) error {
	data, err := os.ReadFile(path)
	if err != nil {
		return err
	}
	if err := json.Unmarshal(data, value); err != nil {
		return fmt.Errorf("decode %s: %w", path, err)
	}
	return nil
}

func readPinned(path, digest string) ([]byte, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}
	if actual := fmt.Sprintf("%x", sha256.Sum256(data)); actual != digest {
		return nil, fmt.Errorf("%s SHA-256 mismatch: want %s, got %s", path, digest, actual)
	}
	return data, nil
}

func definitionNames(directory string) (map[string]bool, error) {
	files, err := os.ReadDir(directory)
	if err != nil {
		return nil, fmt.Errorf("read pinned behavior definitions (run make assets): %w", err)
	}
	names := make(map[string]bool, len(files))
	for _, file := range files {
		if file.IsDir() || filepath.Ext(file.Name()) != ".json" {
			continue
		}
		var definition struct {
			Block struct {
				Description struct {
					Identifier string `json:"identifier"`
				} `json:"description"`
			} `json:"minecraft:block"`
		}
		if err := readJSON(filepath.Join(directory, file.Name()), &definition); err != nil {
			return nil, err
		}
		name := definition.Block.Description.Identifier
		if !strings.HasPrefix(name, "minecraft:") || strings.ContainsAny(name, "\t\r\n ") {
			return nil, fmt.Errorf("invalid vanilla block identifier %q", name)
		}
		if names[name] {
			return nil, fmt.Errorf("duplicate vanilla block definition %q", name)
		}
		names[name] = true
	}
	if len(names) == 0 {
		return nil, errors.New("pinned behavior pack contains no block definitions")
	}
	return names, nil
}

func decodeStates(data []byte, count int) ([]sourceEntry, error) {
	decoder := nbt.NewDecoder(bytes.NewReader(data))
	entries := make([]sourceEntry, 0, count)
	for index := range count {
		var state struct {
			Name       string         `nbt:"name"`
			Properties map[string]any `nbt:"states"`
			Version    int32          `nbt:"version"`
		}
		if err := decoder.Decode(&state); err != nil {
			return nil, fmt.Errorf("decode pinned block state %d: %w", index, err)
		}
		hash := fnv.New64()
		_, _ = hash.Write([]byte(state.Name))
		entries = append(entries, sourceEntry{name: state.Name, hash: hash.Sum64()})
	}
	return entries, nil
}

func buildTable(entries []sourceEntry, names map[string]bool, registryHash string) ([]byte, error) {
	entries = slices.Clone(entries)
	slices.SortStableFunc(entries, compareEntries)
	var out bytes.Buffer
	out.WriteString("# cinnabar.server-defined-blocks.v1\n")
	fmt.Fprintf(&out, "# registry_sha256=%s\n", registryHash)
	out.WriteString("# block_name\tfirst_internal_id\tstate_count\n")
	seen := make(map[string]bool, len(names))
	for start := 0; start < len(entries); {
		name := entries[start].name
		end := start + 1
		for end < len(entries) && entries[end].name == name {
			end++
		}
		if names[name] {
			if seen[name] {
				return nil, fmt.Errorf("block %q has noncontiguous canonical states", name)
			}
			seen[name] = true
			fmt.Fprintf(&out, "%s\t%d\t%d\n", name, start, end-start)
		}
		start = end
	}
	for name := range names {
		if !seen[name] {
			return nil, fmt.Errorf("vanilla definition %q is missing from the projection source", name)
		}
	}
	return out.Bytes(), nil
}

func compareEntries(a, b sourceEntry) int {
	if a.hash < b.hash {
		return -1
	}
	if a.hash > b.hash {
		return 1
	}
	return strings.Compare(a.name, b.name)
}
