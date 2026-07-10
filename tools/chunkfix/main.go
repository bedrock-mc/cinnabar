package main

import (
	"bytes"
	"encoding/binary"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"runtime/debug"
	"slices"
	"strconv"
	"strings"

	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/world/chunk"
)

const (
	// Keep these provenance values in lockstep with tools/chunkfix/go.mod. The
	// generated manifest records them so a fixture can always be traced back to
	// the exact Dragonfly encoder implementation that produced it.
	sourceModule  = "github.com/df-mc/dragonfly"
	sourceVersion = "v0.10.15-0.20260709170650-b85c56ffea6b"
	sourceCommit  = "b85c56ffea6b306798a935f14cc941c76618be52"

	overworldMinY  = -64
	overworldMaxY  = 319
	subChunkIndex  = 0
	subChunkY      = -4
	fakeBlockCount = 4096
)

type sourceDescriptor struct {
	Module  string `json:"module"`
	Version string `json:"version"`
	Commit  string `json:"commit"`
}

type storageDescriptor struct {
	BitsPerIndex uint8 `json:"bits_per_index"`
	PaletteLen   int   `json:"palette_len"`
}

type sample struct {
	Name      string `json:"name"`
	Layer     uint8  `json:"layer"`
	X         uint8  `json:"x"`
	Y         uint8  `json:"y"`
	Z         uint8  `json:"z"`
	RuntimeID uint32 `json:"runtime_id"`
}

type manifestFixture struct {
	File     string              `json:"file"`
	Version  uint8               `json:"version"`
	YIndex   int8                `json:"y_index"`
	Storages []storageDescriptor `json:"storages"`
	Samples  []sample            `json:"samples"`
}

type fixtureManifest struct {
	Source   sourceDescriptor  `json:"source"`
	Fixtures []manifestFixture `json:"fixtures"`
}

type fixtureSpec struct {
	file     string
	storages []storageDescriptor
	samples  []sample
	populate func(*chunk.Chunk)
}

func main() {
	out := flag.String("out", "", "directory to write sub-chunk fixtures")
	flag.Parse()
	if *out == "" {
		fmt.Fprintln(os.Stderr, "chunkfix: -out is required")
		os.Exit(2)
	}
	if err := generate(*out); err != nil {
		fmt.Fprintf(os.Stderr, "chunkfix: %v\n", err)
		os.Exit(1)
	}
}

func generate(out string) error {
	if out == "" {
		return errors.New("output directory is empty")
	}
	if chunk.SubChunkVersion != 9 {
		return fmt.Errorf("Dragonfly sub-chunk version drift: got %d, want 9", chunk.SubChunkVersion)
	}
	if err := verifySourcePin(); err != nil {
		return err
	}
	if err := os.MkdirAll(out, 0o755); err != nil {
		return fmt.Errorf("create output directory: %w", err)
	}

	manifest := fixtureManifest{
		Source: sourceDescriptor{
			Module:  sourceModule,
			Version: sourceVersion,
			Commit:  sourceCommit,
		},
		Fixtures: make([]manifestFixture, 0, len(fixtureSpecs())),
	}
	for _, spec := range fixtureSpecs() {
		c := chunk.New(fakeBlockRegistry{}, cube.Range{overworldMinY, overworldMaxY})
		spec.populate(c)

		// Compact before encoding so the uniform and filled-pattern fixtures prove
		// that unused air entries do not survive in their runtime palettes.
		c.Compact()
		if err := verifySamples(c, spec); err != nil {
			return err
		}

		// This is intentionally the production network encoder rather than a local
		// reimplementation. Keeping the call explicit is the fixture provenance.
		encoded := chunk.EncodeSubChunk(c, chunk.NetworkEncoding, subChunkIndex)
		version, yIndex, storages, err := inspectSubChunk(encoded)
		if err != nil {
			return fmt.Errorf("inspect %s: %w", spec.file, err)
		}
		if version != chunk.SubChunkVersion || yIndex != subChunkY {
			return fmt.Errorf(
				"%s header = version %d/y %d, want %d/%d",
				spec.file, version, yIndex, chunk.SubChunkVersion, subChunkY,
			)
		}
		if !slices.Equal(storages, spec.storages) {
			return fmt.Errorf("%s storages = %+v, want %+v", spec.file, storages, spec.storages)
		}

		path := filepath.Join(out, spec.file)
		if err := os.WriteFile(path, encoded, 0o644); err != nil {
			return fmt.Errorf("write %s: %w", path, err)
		}
		manifest.Fixtures = append(manifest.Fixtures, manifestFixture{
			File:     spec.file,
			Version:  version,
			YIndex:   yIndex,
			Storages: slices.Clone(storages),
			Samples:  slices.Clone(spec.samples),
		})
	}

	encodedManifest, err := json.MarshalIndent(manifest, "", "  ")
	if err != nil {
		return fmt.Errorf("encode manifest: %w", err)
	}
	encodedManifest = append(encodedManifest, '\n')
	path := filepath.Join(out, "manifest.json")
	if err := os.WriteFile(path, encodedManifest, 0o644); err != nil {
		return fmt.Errorf("write %s: %w", path, err)
	}
	return nil
}

func verifySourcePin() error {
	build, ok := debug.ReadBuildInfo()
	if !ok {
		return errors.New("read Go build dependency information")
	}
	for _, dependency := range build.Deps {
		if dependency.Path != sourceModule {
			continue
		}
		actual := dependency
		if dependency.Replace != nil {
			actual = dependency.Replace
		}
		if actual.Version != sourceVersion {
			return fmt.Errorf(
				"Dragonfly dependency drift: built with %q, manifest pin is %q",
				actual.Version, sourceVersion,
			)
		}
		separator := strings.LastIndexByte(sourceVersion, '-')
		if separator == -1 || separator == len(sourceVersion)-1 ||
			!strings.HasPrefix(sourceCommit, sourceVersion[separator+1:]) {
			return fmt.Errorf("Dragonfly commit %q does not match version %q", sourceCommit, sourceVersion)
		}
		return nil
	}
	return fmt.Errorf("build does not contain required dependency %s", sourceModule)
}

func fixtureSpecs() []fixtureSpec {
	specs := []fixtureSpec{
		{
			file:     "uniform_non_air.bin",
			storages: []storageDescriptor{{BitsPerIndex: 0, PaletteLen: 1}},
			samples: []sample{
				{Name: "corner", Layer: 0, X: 0, Y: 0, Z: 0, RuntimeID: 42},
				{Name: "centre", Layer: 0, X: 8, Y: 8, Z: 8, RuntimeID: 42},
				{Name: "opposite_corner", Layer: 0, X: 15, Y: 15, Z: 15, RuntimeID: 42},
			},
			populate: func(c *chunk.Chunk) {
				fillLayer(c, 0, func(_, _, _ uint8) uint32 { return 42 })
			},
		},
		{
			file:     "checkerboard.bin",
			storages: []storageDescriptor{{BitsPerIndex: 1, PaletteLen: 2}},
			samples: []sample{
				{Name: "even", Layer: 0, X: 0, Y: 0, Z: 0, RuntimeID: 7},
				{Name: "odd_x", Layer: 0, X: 1, Y: 0, Z: 0, RuntimeID: 11},
				{Name: "odd_xyz", Layer: 0, X: 15, Y: 15, Z: 15, RuntimeID: 11},
			},
			populate: func(c *chunk.Chunk) {
				fillLayer(c, 0, func(x, y, z uint8) uint32 {
					if (x+y+z)&1 == 0 {
						return 7
					}
					return 11
				})
			},
		},
		{
			file:     "vertical_layers.bin",
			storages: []storageDescriptor{{BitsPerIndex: 4, PaletteLen: 16}},
			samples: []sample{
				{Name: "bottom", Layer: 0, X: 3, Y: 0, Z: 12, RuntimeID: 100},
				{Name: "middle", Layer: 0, X: 8, Y: 7, Z: 8, RuntimeID: 107},
				{Name: "top", Layer: 0, X: 12, Y: 15, Z: 3, RuntimeID: 115},
			},
			populate: func(c *chunk.Chunk) {
				fillLayer(c, 0, func(_, y, _ uint8) uint32 { return 100 + uint32(y) })
			},
		},
		{
			file: "two_storage_layers.bin",
			storages: []storageDescriptor{
				{BitsPerIndex: 0, PaletteLen: 1},
				{BitsPerIndex: 1, PaletteLen: 2},
			},
			samples: []sample{
				{Name: "base", Layer: 0, X: 4, Y: 5, Z: 6, RuntimeID: 21},
				{Name: "overlay_even", Layer: 1, X: 0, Y: 0, Z: 0, RuntimeID: 22},
				{Name: "overlay_odd", Layer: 1, X: 0, Y: 1, Z: 0, RuntimeID: 23},
			},
			populate: func(c *chunk.Chunk) {
				fillLayer(c, 0, func(_, _, _ uint8) uint32 { return 21 })
				fillLayer(c, 1, func(x, y, z uint8) uint32 {
					if (x+y+z)&1 == 0 {
						return 22
					}
					return 23
				})
			},
		},
	}

	// These are the smallest palette lengths that force every legal Bedrock
	// storage width. They deliberately straddle each Dragonfly resize boundary.
	for _, width := range []struct {
		bits       uint8
		paletteLen int
	}{
		{bits: 1, paletteLen: 2},
		{bits: 2, paletteLen: 3},
		{bits: 3, paletteLen: 5},
		{bits: 4, paletteLen: 9},
		{bits: 5, paletteLen: 17},
		{bits: 6, paletteLen: 33},
		{bits: 8, paletteLen: 65},
		{bits: 16, paletteLen: 257},
	} {
		bits, paletteLen := width.bits, width.paletteLen
		uniqueCount := paletteLen - 1
		valuesPerWord := 32 / int(bits)
		wordBoundary := (uniqueCount + valuesPerWord - 1) / valuesPerWord * valuesPerWord
		beforeBoundary := wordBoundary - 1
		beforeBoundaryRuntimeID := uint32(1)
		if beforeBoundary < uniqueCount {
			beforeBoundaryRuntimeID = uint32(beforeBoundary + 1)
		}

		samples := []sample{sampleAt("first_non_air", 0, 0, 1)}
		if paletteLen > 2 {
			samples = append(samples, sampleAt("last_non_air", 0, uniqueCount-1, uint32(uniqueCount)))
		}
		samples = append(samples,
			sampleAt("end_word_before_boundary", 0, beforeBoundary, beforeBoundaryRuntimeID),
			sampleAt("start_word_after_boundary", 0, wordBoundary, uint32(uniqueCount)),
			sampleAt("air_after_boundary", 0, wordBoundary+1, 0),
			sampleAt("last_block", 0, 4095, 1),
		)
		specs = append(specs, fixtureSpec{
			file:     fmt.Sprintf("bits_%d.bin", bits),
			storages: []storageDescriptor{{BitsPerIndex: bits, PaletteLen: paletteLen}},
			samples:  samples,
			populate: func(c *chunk.Chunk) {
				for runtimeID := 1; runtimeID <= uniqueCount; runtimeID++ {
					x, y, z := coordinateForOrdinal(runtimeID - 1)
					c.SetBlock(x, overworldMinY+int16(y), z, 0, uint32(runtimeID))
				}
				if beforeBoundary >= uniqueCount {
					x, y, z := coordinateForOrdinal(beforeBoundary)
					c.SetBlock(x, overworldMinY+int16(y), z, 0, 1)
				}
				x, y, z := coordinateForOrdinal(wordBoundary)
				c.SetBlock(x, overworldMinY+int16(y), z, 0, uint32(uniqueCount))
				x, y, z = coordinateForOrdinal(4095)
				c.SetBlock(x, overworldMinY+int16(y), z, 0, 1)
			},
		})
	}
	return specs
}

func fillLayer(c *chunk.Chunk, layer uint8, runtimeID func(x, y, z uint8) uint32) {
	for x := uint8(0); x < 16; x++ {
		for z := uint8(0); z < 16; z++ {
			for y := uint8(0); y < 16; y++ {
				c.SetBlock(x, overworldMinY+int16(y), z, layer, runtimeID(x, y, z))
			}
		}
	}
}

func sampleAt(name string, layer uint8, ordinal int, runtimeID uint32) sample {
	x, y, z := coordinateForOrdinal(ordinal)
	return sample{Name: name, Layer: layer, X: x, Y: y, Z: z, RuntimeID: runtimeID}
}

func coordinateForOrdinal(ordinal int) (x, y, z uint8) {
	return uint8((ordinal >> 8) & 15), uint8(ordinal & 15), uint8((ordinal >> 4) & 15)
}

func verifySamples(c *chunk.Chunk, spec fixtureSpec) error {
	for _, want := range spec.samples {
		got := c.Block(want.X, overworldMinY+int16(want.Y), want.Z, want.Layer)
		if got != want.RuntimeID {
			return fmt.Errorf(
				"%s sample %q at layer %d (%d,%d,%d) = %d, want %d",
				spec.file, want.Name, want.Layer, want.X, want.Y, want.Z, got, want.RuntimeID,
			)
		}
	}
	return nil
}

func inspectSubChunk(encoded []byte) (uint8, int8, []storageDescriptor, error) {
	if len(encoded) < 3 {
		return 0, 0, nil, fmt.Errorf("header truncated: got %d bytes", len(encoded))
	}
	version, storageCount, yIndex := encoded[0], int(encoded[1]), int8(encoded[2])
	r := bytes.NewReader(encoded[3:])
	storages := make([]storageDescriptor, 0, storageCount)
	for storageIndex := 0; storageIndex < storageCount; storageIndex++ {
		header, err := r.ReadByte()
		if err != nil {
			return 0, 0, nil, fmt.Errorf("storage %d header: %w", storageIndex, err)
		}
		if header&1 != 1 {
			return 0, 0, nil, fmt.Errorf("storage %d is not network encoded", storageIndex)
		}
		bits := header >> 1
		if !validBitsPerIndex(bits) {
			return 0, 0, nil, fmt.Errorf("storage %d has unsupported width %d", storageIndex, bits)
		}
		wordCount := 0
		if bits != 0 {
			indicesPerWord := 32 / int(bits)
			wordCount = (4096 + indicesPerWord - 1) / indicesPerWord
		}
		if _, err := r.Seek(int64(wordCount*4), io.SeekCurrent); err != nil {
			return 0, 0, nil, fmt.Errorf("storage %d indices: %w", storageIndex, err)
		}

		paletteLen := 1
		if bits != 0 {
			paletteLen, err = readPositiveVarint32(r)
			if err != nil {
				return 0, 0, nil, fmt.Errorf("storage %d palette length: %w", storageIndex, err)
			}
		}
		for paletteIndex := 0; paletteIndex < paletteLen; paletteIndex++ {
			if _, err := binary.ReadUvarint(r); err != nil {
				return 0, 0, nil, fmt.Errorf("storage %d palette entry %d: %w", storageIndex, paletteIndex, err)
			}
		}
		storages = append(storages, storageDescriptor{BitsPerIndex: bits, PaletteLen: paletteLen})
	}
	if r.Len() != 0 {
		return 0, 0, nil, fmt.Errorf("%d trailing bytes", r.Len())
	}
	return version, yIndex, storages, nil
}

func readPositiveVarint32(r *bytes.Reader) (int, error) {
	u, err := binary.ReadUvarint(r)
	if err != nil {
		return 0, err
	}
	if u > uint64(^uint32(0)) {
		return 0, fmt.Errorf("encoded value %d exceeds uint32", u)
	}
	v := int32(uint32(u>>1)) ^ -int32(u&1)
	if v <= 0 {
		return 0, fmt.Errorf("decoded value %d is not positive", v)
	}
	return int(v), nil
}

func validBitsPerIndex(bits uint8) bool {
	switch bits {
	case 0, 1, 2, 3, 4, 5, 6, 8, 16:
		return true
	default:
		return false
	}
}

// fakeBlockRegistry is deliberately data-free: network palettes encode runtime
// IDs directly, but Dragonfly requires a complete BlockRegistry to own a Chunk.
// Identity hashes and deterministic synthetic state names keep the implementation
// valid if a future diagnostic uses the non-network registry methods.
type fakeBlockRegistry struct{}

func (fakeBlockRegistry) BlockCount() int      { return fakeBlockCount }
func (fakeBlockRegistry) AirRuntimeID() uint32 { return 0 }

func (fakeBlockRegistry) RuntimeIDToState(runtimeID uint32) (string, map[string]any, bool) {
	if runtimeID >= fakeBlockCount {
		return "", nil, false
	}
	return fmt.Sprintf("fixture:block_%d", runtimeID), map[string]any{}, true
}

func (fakeBlockRegistry) StateToRuntimeID(name string, _ map[string]any) (uint32, bool) {
	const prefix = "fixture:block_"
	if !strings.HasPrefix(name, prefix) {
		return 0, false
	}
	v, err := strconv.ParseUint(strings.TrimPrefix(name, prefix), 10, 32)
	return uint32(v), err == nil && v < fakeBlockCount
}

func (fakeBlockRegistry) FilteringBlock(runtimeID uint32) uint8 {
	if runtimeID == 0 {
		return 0
	}
	return 15
}

func (fakeBlockRegistry) LightBlock(uint32) uint8           { return 0 }
func (fakeBlockRegistry) RandomTickBlock(uint32) bool       { return false }
func (fakeBlockRegistry) NBTBlock(uint32) bool              { return false }
func (fakeBlockRegistry) LiquidDisplacingBlock(uint32) bool { return false }
func (fakeBlockRegistry) LiquidBlock(uint32) bool           { return false }
func (fakeBlockRegistry) HashToRuntimeID(hash uint32) (uint32, bool) {
	return hash, hash < fakeBlockCount
}
func (fakeBlockRegistry) RuntimeIDToHash(runtimeID uint32) (uint32, bool) {
	return runtimeID, runtimeID < fakeBlockCount
}
