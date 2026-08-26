package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/binary"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
)

// rekeyEncodeTestBREG encodes synthetic registry records as a structurally
// valid BREG1003 payload stamped with the requested protocol.
func rekeyEncodeTestBREG(t *testing.T, protocol uint32, records []Record) []byte {
	t.Helper()
	metadata := metadataForRecords(records)
	metadata.Protocol = protocol
	encoded, err := encodeWithMetadata(metadata, records)
	if err != nil {
		t.Fatal(err)
	}
	return encoded
}

// rekeyTestEntry builds one 25-byte fallback inventory entry whose stored
// fingerprint is derived from the supplied canonical identity.
func rekeyTestEntry(t *testing.T, hash uint32, name, state string, min, max [3]int16, alpha byte) []byte {
	t.Helper()
	entry := make([]byte, fallbackEntryBytes)
	binary.LittleEndian.PutUint32(entry[0:4], hash)
	binary.LittleEndian.PutUint64(entry[4:12], fallbackIdentityFingerprint(name, []byte(state)))
	for axis := 0; axis < 3; axis++ {
		binary.LittleEndian.PutUint16(entry[12+axis*2:], uint16(min[axis]))
		binary.LittleEndian.PutUint16(entry[18+axis*2:], uint16(max[axis]))
	}
	entry[24] = alpha
	return entry
}

// rekeyTestTable builds a synthetic CVFB1001 table around the given entries.
func rekeyTestTable(entries ...[]byte) []byte {
	table := make([]byte, 0, fallbackHeaderBytes+len(entries)*fallbackEntryBytes)
	table = append(table, "CVFB1001"...)
	table = binary.LittleEndian.AppendUint32(table, 1)
	table = binary.LittleEndian.AppendUint32(table, uint32(len(entries)))
	for _, entry := range entries {
		table = append(table, entry...)
	}
	return table
}

func rekeyTestRecord(seq, hash uint32, name, state string) Record {
	return Record{SequentialID: seq, NetworkHash: hash, Name: name, StateJSON: []byte(state), Provenance: ProvenanceDragonfly}
}

func TestRekeyFallbackRealRegistriesAreFullFidelity(t *testing.T) {
	root := filepath.Join("..", "..")
	legacyBytes, err := os.ReadFile(filepath.Join(root, "crates", "assets", "data", "block-registry-v1001.bin"))
	if err != nil {
		t.Fatal(err)
	}
	_, legacy, err := decodeBREGRecords(legacyBytes, registryProtocol)
	if err != nil {
		t.Fatal(err)
	}
	currentBytes, err := os.ReadFile(filepath.Join(root, "crates", "assets", "data", "block-registry-v2168.bin"))
	if err != nil {
		t.Fatal(err)
	}
	_, current, err := decodeBREGRecords(currentBytes, v2168BlockProtocol)
	if err != nil {
		t.Fatal(err)
	}
	input, err := os.ReadFile(filepath.Join(root, "crates", "asset-compiler", "data", "vanilla-fallback-v1001.bin"))
	if err != nil {
		t.Fatal(err)
	}
	output, stats, err := rekeyFallbackInventory(input, legacy, current)
	if err != nil {
		t.Fatal(err)
	}
	if stats.InputEntries != 2_031 || stats.OutputEntries != 2_031 || stats.ReservedExcluded != 0 {
		t.Fatalf("rekey counts = in %d out %d reserved-excluded %d", stats.InputEntries, stats.OutputEntries, stats.ReservedExcluded)
	}
	count, err := parseFallbackInventoryHeader(output)
	if err != nil {
		t.Fatal(err)
	}
	if count != 2_031 {
		t.Fatalf("output header count = %d", count)
	}
	currentByHash := make(map[uint32]Record, len(current))
	for _, record := range current {
		currentByHash[record.NetworkHash] = record
	}
	inputPayloads := make(map[string]int)
	for index := 0; index < stats.InputEntries; index++ {
		start := fallbackHeaderBytes + index*fallbackEntryBytes
		entry := input[start : start+fallbackEntryBytes]
		key := fmt.Sprintf("%016x/%x", binary.LittleEndian.Uint64(entry[4:12]), entry[12:25])
		inputPayloads[key]++
	}
	outputPayloads := make(map[string]int)
	previousHash := uint32(0)
	for index := 0; index < count; index++ {
		start := fallbackHeaderBytes + index*fallbackEntryBytes
		entry := output[start : start+fallbackEntryBytes]
		hash := binary.LittleEndian.Uint32(entry[0:4])
		if index > 0 && hash <= previousHash {
			t.Fatalf("output entries are not strictly sorted by new network hash at %d", index)
		}
		previousHash = hash
		record, ok := currentByHash[hash]
		if !ok {
			t.Fatalf("output entry %d has no v2168 registry record", index)
		}
		if fingerprint := fallbackIdentityFingerprint(record.Name, record.StateJSON); fingerprint != binary.LittleEndian.Uint64(entry[4:12]) {
			t.Fatalf("output entry %d fingerprint does not match the v2168 identity %s", index, record.Name)
		}
		key := fmt.Sprintf("%016x/%x", binary.LittleEndian.Uint64(entry[4:12]), entry[12:25])
		outputPayloads[key]++
	}
	if len(inputPayloads) != len(outputPayloads) {
		t.Fatalf("payload identity multiset changed: %d distinct in, %d distinct out", len(inputPayloads), len(outputPayloads))
	}
	for key, want := range inputPayloads {
		if outputPayloads[key] != want {
			t.Fatalf("payload identity %s preserved %d times, want %d", key, outputPayloads[key], want)
		}
	}
	if stats.DistinctNames != 335 || stats.ZeroVolume != 5 {
		t.Fatalf("preserved-envelope counts = names %d zero-volume %d", stats.DistinctNames, stats.ZeroVolume)
	}
	if !bytes.Equal(input, output) {
		t.Fatal("current-corpus rekey did not reproduce the input bytes exactly")
	}
	regenerated, regeneratedStats, err := rekeyFallbackInventory(input, legacy, current)
	if err != nil || regeneratedStats != stats {
		t.Fatalf("regeneration diverged: %v / %+v vs %+v", err, regeneratedStats, stats)
	}
	if !bytes.Equal(output, regenerated) {
		t.Fatal("rekeyed inventory regeneration is not byte-deterministic")
	}
}

// TestCheckedInV2168FallbackInventoryIsRekeyedAndHashBound pins the committed
// v2168 fallback artifact in the biome-pin precedent style: its exact bytes
// must match the tracked sidecar digest and must be reproducible byte-for-byte
// by rekeying the checked-in v1001 input against both checked-in registries,
// so a stale or hand-edited artifact fails instead of drifting silently.
func TestCheckedInV2168FallbackInventoryIsRekeyedAndHashBound(t *testing.T) {
	root := filepath.Join("..", "..")
	inventory, err := os.ReadFile(filepath.Join(root, "crates", "assets", "data", "vanilla-fallback-v2168.bin"))
	if err != nil {
		t.Fatal(err)
	}
	sidecar, err := os.ReadFile(filepath.Join(root, "crates", "assets", "data", "vanilla-fallback-v2168.sha256"))
	if err != nil {
		t.Fatal(err)
	}
	digest := fmt.Sprintf("%x\n", sha256.Sum256(inventory))
	if digest != string(sidecar) {
		t.Fatalf("checked-in v2168 fallback SHA-256 %s does not match sidecar %s", digest, sidecar)
	}
	legacyBytes, err := os.ReadFile(filepath.Join(root, "crates", "assets", "data", "block-registry-v1001.bin"))
	if err != nil {
		t.Fatal(err)
	}
	_, legacyRecords, err := decodeBREGRecords(legacyBytes, registryProtocol)
	if err != nil {
		t.Fatal(err)
	}
	currentBytes, err := os.ReadFile(filepath.Join(root, "crates", "assets", "data", "block-registry-v2168.bin"))
	if err != nil {
		t.Fatal(err)
	}
	_, currentRecords, err := decodeBREGRecords(currentBytes, v2168BlockProtocol)
	if err != nil {
		t.Fatal(err)
	}
	input, err := os.ReadFile(filepath.Join(root, "crates", "asset-compiler", "data", "vanilla-fallback-v1001.bin"))
	if err != nil {
		t.Fatal(err)
	}
	rekeyed, _, err := rekeyFallbackInventory(input, legacyRecords, currentRecords)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(inventory, rekeyed) {
		t.Fatal("checked-in v2168 fallback inventory is not a fresh byte-exact rekey of the v1001 input")
	}
}

func TestRekeyFallbackRejectsMutatedFingerprintNamingTheEntry(t *testing.T) {
	legacy := []Record{rekeyTestRecord(0, 100, "minecraft:brick_wall", `{}`)}
	current := []Record{rekeyTestRecord(0, 200, "minecraft:brick_wall", `{}`)}
	input := rekeyTestTable(func() []byte {
		entry := rekeyTestEntry(t, 100, "minecraft:brick_wall", `{}`, [3]int16{0, 0, 0}, [3]int16{16, 16, 16}, 1)
		fingerprint := binary.LittleEndian.Uint64(entry[4:12])
		binary.LittleEndian.PutUint64(entry[4:12], fingerprint^1)
		return entry
	}())
	_, _, err := rekeyFallbackInventory(input, legacy, current)
	if err == nil {
		t.Fatal("mutated identity fingerprint was accepted")
	}
	if !strings.Contains(err.Error(), "minecraft:brick_wall") {
		t.Fatalf("corruption error does not name the entry: %v", err)
	}
}

func TestRekeyFallbackRejectsUnmatchedEntriesSorted(t *testing.T) {
	legacyState := `{"k":{"type":"int","value":1}}`
	legacy := []Record{
		rekeyTestRecord(0, 10, "minecraft:kept", legacyState),
		rekeyTestRecord(1, 11, "minecraft:dropped_new", `{}`),
	}
	current := []Record{rekeyTestRecord(0, 900, "minecraft:kept", legacyState)}
	input := rekeyTestTable(
		rekeyTestEntry(t, 777, "unmatched-a", `{}`, [3]int16{}, [3]int16{16, 16, 16}, 1),
		rekeyTestEntry(t, 500, "unmatched-b", `{}`, [3]int16{}, [3]int16{16, 16, 16}, 1),
		rekeyTestEntry(t, 10, "minecraft:kept", legacyState, [3]int16{}, [3]int16{16, 16, 16}, 1),
		rekeyTestEntry(t, 11, "minecraft:dropped_new", `{}`, [3]int16{}, [3]int16{16, 16, 16}, 1),
	)
	_, _, err := rekeyFallbackInventory(input, legacy, current)
	if err == nil {
		t.Fatal("unmatched entries were accepted")
	}
	listing := []string{"minecraft:dropped_new {}", "network hash 0x1f4", "network hash 0x309"}
	previous := -1
	for _, name := range listing {
		at := strings.Index(err.Error(), name)
		if at < 0 {
			t.Fatalf("unmatched error is missing %q: %v", name, err)
		}
		if at < previous {
			t.Fatalf("unmatched listing is not sorted at %q: %v", name, err)
		}
		previous = at
	}
	if !strings.Contains(err.Error(), "3 unmatched") {
		t.Fatalf("unmatched error does not report the count: %v", err)
	}
}

func TestRekeyFallbackExcludesReservedCollisionsAndCountsThem(t *testing.T) {
	reservedState := string(reservedStateJSON(7))
	legacy := []Record{
		rekeyTestRecord(7, 100, retailReservedName, reservedState),
		rekeyTestRecord(8, 101, "minecraft:kept", `{}`),
	}
	current := []Record{
		rekeyTestRecord(7, 200, retailReservedName, reservedState),
		rekeyTestRecord(8, 201, "minecraft:kept", `{}`),
	}
	input := rekeyTestTable(
		rekeyTestEntry(t, 100, retailReservedName, reservedState, [3]int16{}, [3]int16{16, 16, 16}, 1),
		rekeyTestEntry(t, 101, "minecraft:kept", `{}`, [3]int16{1, 2, 3}, [3]int16{15, 14, 13}, 2),
	)
	output, stats, err := rekeyFallbackInventory(input, legacy, current)
	if err != nil {
		t.Fatal(err)
	}
	if stats.ReservedExcluded != 1 || stats.InputEntries != 2 || stats.OutputEntries != 1 {
		t.Fatalf("reserved exclusion stats = %+v", stats)
	}
	count, err := parseFallbackInventoryHeader(output)
	if err != nil {
		t.Fatal(err)
	}
	if count != 1 {
		t.Fatalf("output header count = %d", count)
	}
	entry := output[fallbackHeaderBytes : fallbackHeaderBytes+fallbackEntryBytes]
	if hash := binary.LittleEndian.Uint32(entry[0:4]); hash != 201 {
		t.Fatalf("surviving entry hash = %#x", hash)
	}
	if !bytes.Equal(entry[12:25], input[fallbackHeaderBytes+fallbackEntryBytes+12:]) {
		t.Fatal("surviving entry payload was not preserved verbatim")
	}
}

func TestRekeyFallbackRejectsWrongVersionBREGs(t *testing.T) {
	dir := t.TempDir()
	legacyRecords := []Record{rekeyTestRecord(0, 100, "minecraft:kept", `{}`)}
	currentRecords := []Record{rekeyTestRecord(0, 200, "minecraft:kept", `{}`)}
	inputPath := filepath.Join(dir, "input.bin")
	legacyPath := filepath.Join(dir, "legacy.bin")
	currentPath := filepath.Join(dir, "current.bin")
	outputPath := filepath.Join(dir, "out", "rekeyed.bin")
	if err := os.WriteFile(inputPath, rekeyTestTable(rekeyTestEntry(t, 100, "minecraft:kept", `{}`, [3]int16{}, [3]int16{16, 16, 16}, 1)), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(legacyPath, rekeyEncodeTestBREG(t, v2168BlockProtocol, legacyRecords), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(currentPath, rekeyEncodeTestBREG(t, registryProtocol, currentRecords), 0o644); err != nil {
		t.Fatal(err)
	}
	_, err := writeRekeyedFallback(inputPath, legacyPath, currentPath, outputPath, "")
	if err == nil || !strings.Contains(err.Error(), "protocol-1001") {
		t.Fatalf("v2168 bytes in the legacy slot were accepted: %v", err)
	}
	if err := os.WriteFile(legacyPath, rekeyEncodeTestBREG(t, registryProtocol, legacyRecords), 0o644); err != nil {
		t.Fatal(err)
	}
	_, err = writeRekeyedFallback(inputPath, legacyPath, currentPath, outputPath, "")
	if err == nil || !strings.Contains(err.Error(), "protocol-2168") {
		t.Fatalf("protocol-1001 bytes in the new slot were accepted: %v", err)
	}
}

func TestRekeyFallbackManifestMirrorsSourceSchema(t *testing.T) {
	dir := t.TempDir()
	legacyRecords := []Record{
		rekeyTestRecord(7, 100, retailReservedName, string(reservedStateJSON(7))),
		rekeyTestRecord(8, 101, "minecraft:kept", `{}`),
	}
	currentRecords := []Record{
		rekeyTestRecord(7, 200, retailReservedName, string(reservedStateJSON(7))),
		rekeyTestRecord(8, 201, "minecraft:kept", `{}`),
	}
	inputPath := filepath.Join(dir, "input.bin")
	legacyPath := filepath.Join(dir, "legacy.bin")
	currentPath := filepath.Join(dir, "current.bin")
	outputPath := filepath.Join(dir, "out", "rekeyed.bin")
	manifestPath := filepath.Join(dir, "out", "manifest.json")
	if err := os.WriteFile(inputPath, rekeyTestTable(
		rekeyTestEntry(t, 100, retailReservedName, string(reservedStateJSON(7)), [3]int16{}, [3]int16{16, 16, 16}, 1),
		rekeyTestEntry(t, 101, "minecraft:kept", `{}`, [3]int16{8, 0, 0}, [3]int16{8, 16, 16}, 2),
	), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(legacyPath, rekeyEncodeTestBREG(t, registryProtocol, legacyRecords), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(currentPath, rekeyEncodeTestBREG(t, v2168BlockProtocol, currentRecords), 0o644); err != nil {
		t.Fatal(err)
	}
	if _, err := writeRekeyedFallback(inputPath, legacyPath, currentPath, outputPath, manifestPath); err != nil {
		t.Fatal(err)
	}
	payload, err := os.ReadFile(manifestPath)
	if err != nil {
		t.Fatal(err)
	}
	var manifest vanillaFallbackSourceManifest
	decoder := json.NewDecoder(bytes.NewReader(payload))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&manifest); err != nil {
		t.Fatalf("manifest does not mirror the source schema: %v", err)
	}
	encoded, err := os.ReadFile(outputPath)
	if err != nil {
		t.Fatal(err)
	}
	newBREG, err := os.ReadFile(currentPath)
	if err != nil {
		t.Fatal(err)
	}
	inputFile, err := os.ReadFile(inputPath)
	if err != nil {
		t.Fatal(err)
	}
	legacyBREG, err := os.ReadFile(legacyPath)
	if err != nil {
		t.Fatal(err)
	}
	if manifest.Schema != "cinnabar-vanilla-fallback-source-v1" || manifest.Protocol != v2168BlockProtocol ||
		manifest.Status != "provisional-vanilla-fallback" {
		t.Fatalf("manifest identity = %+v", manifest)
	}
	if manifest.States != 1 || manifest.Names != 1 || manifest.ZeroVolumeEnvelopes != 1 {
		t.Fatalf("manifest counts = %+v", manifest)
	}
	if manifest.Inventory.SHA256 != fmt.Sprintf("%x", sha256.Sum256(encoded)) || manifest.Inventory.EntryBytes != fallbackEntryBytes {
		t.Fatalf("manifest inventory binding = %+v", manifest.Inventory)
	}
	if manifest.InputInventory.Path != filepath.ToSlash(inputPath) || manifest.InputInventory.SHA256 != fmt.Sprintf("%x", sha256.Sum256(inputFile)) {
		t.Fatalf("manifest input_inventory binding = %+v", manifest.InputInventory)
	}
	if manifest.Registry.SHA256 != fmt.Sprintf("%x", sha256.Sum256(newBREG)) {
		t.Fatalf("manifest registry binding = %+v", manifest.Registry)
	}
	if manifest.LegacyRegistry.Path != filepath.ToSlash(legacyPath) || manifest.LegacyRegistry.SHA256 != fmt.Sprintf("%x", sha256.Sum256(legacyBREG)) {
		t.Fatalf("manifest legacy_registry binding = %+v", manifest.LegacyRegistry)
	}
	if !strings.HasSuffix(string(payload), "}\n") {
		t.Fatal("manifest is not newline terminated")
	}
}

func TestRekeyFallbackRejectsMalformedInventoryHeaders(t *testing.T) {
	legacy := []Record{rekeyTestRecord(0, 100, "minecraft:kept", `{}`)}
	current := []Record{rekeyTestRecord(0, 200, "minecraft:kept", `{}`)}
	valid := rekeyTestTable(rekeyTestEntry(t, 100, "minecraft:kept", `{}`, [3]int16{}, [3]int16{16, 16, 16}, 1))
	tests := []struct {
		name   string
		mutate func(table []byte) []byte
	}{
		{name: "empty input", mutate: func(table []byte) []byte { return nil }},
		{name: "bad magic", mutate: func(table []byte) []byte { table[0] = 'X'; return table }},
		{name: "unsupported version", mutate: func(table []byte) []byte { binary.LittleEndian.PutUint32(table[8:12], 2); return table }},
		{name: "header-only empty table", mutate: func(table []byte) []byte {
			binary.LittleEndian.PutUint32(table[12:16], 0)
			return table[:fallbackHeaderBytes]
		}},
		{name: "lying entry count", mutate: func(table []byte) []byte { binary.LittleEndian.PutUint32(table[12:16], 2); return table }},
		{name: "truncated tail", mutate: func(table []byte) []byte { return table[:len(table)-1] }},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			table := append([]byte(nil), valid...)
			if _, _, err := rekeyFallbackInventory(test.mutate(table), legacy, current); err == nil {
				t.Fatal("malformed fallback inventory header was accepted")
			} else if !strings.Contains(err.Error(), "fallback inventory") {
				t.Fatalf("header error does not name the inventory: %v", err)
			}
		})
	}
}

func TestRekeyFallbackWritesTheOutputChecksumSidecar(t *testing.T) {
	dir := t.TempDir()
	inputPath := filepath.Join(dir, "input.bin")
	legacyPath := filepath.Join(dir, "legacy.bin")
	currentPath := filepath.Join(dir, "current.bin")
	outputPath := filepath.Join(dir, "out", "rekeyed.bin")
	if err := os.WriteFile(inputPath, rekeyTestTable(rekeyTestEntry(t, 100, "minecraft:kept", `{}`, [3]int16{}, [3]int16{16, 16, 16}, 1)), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(legacyPath, rekeyEncodeTestBREG(t, registryProtocol, []Record{rekeyTestRecord(0, 100, "minecraft:kept", `{}`)}), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(currentPath, rekeyEncodeTestBREG(t, v2168BlockProtocol, []Record{rekeyTestRecord(0, 200, "minecraft:kept", `{}`)}), 0o644); err != nil {
		t.Fatal(err)
	}
	if _, err := writeRekeyedFallback(inputPath, legacyPath, currentPath, outputPath, ""); err != nil {
		t.Fatal(err)
	}
	encoded, err := os.ReadFile(outputPath)
	if err != nil {
		t.Fatal(err)
	}
	sidecar, err := os.ReadFile(strings.TrimSuffix(outputPath, filepath.Ext(outputPath)) + ".sha256")
	if err != nil {
		t.Fatal(err)
	}
	if want := fmt.Sprintf("%x\n", sha256.Sum256(encoded)); string(sidecar) != want {
		t.Fatalf("checksum sidecar = %q, want %q", sidecar, want)
	}
}

func TestFallbackRekeyCommandModeIsMutuallyExclusive(t *testing.T) {
	binary := filepath.Join(t.TempDir(), "registrygen")
	if runtime.GOOS == "windows" {
		binary += ".exe"
	}
	build := exec.Command("go", "build", "-o", binary, ".")
	if output, err := build.CombinedOutput(); err != nil {
		t.Fatalf("build registrygen: %v\n%s", err, output)
	}
	dir := t.TempDir()
	inputPath := filepath.Join(dir, "input.bin")
	legacyPath := filepath.Join(dir, "legacy.bin")
	currentPath := filepath.Join(dir, "current.bin")
	outputPath := filepath.Join(dir, "out", "rekeyed.bin")
	if err := os.WriteFile(inputPath, rekeyTestTable(rekeyTestEntry(t, 100, "minecraft:kept", `{}`, [3]int16{}, [3]int16{16, 16, 16}, 1)), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(legacyPath, rekeyEncodeTestBREG(t, registryProtocol, []Record{rekeyTestRecord(0, 100, "minecraft:kept", `{}`)}), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(currentPath, rekeyEncodeTestBREG(t, v2168BlockProtocol, []Record{rekeyTestRecord(0, 200, "minecraft:kept", `{}`)}), 0o644); err != nil {
		t.Fatal(err)
	}
	rekeyArgs := []string{"-fallback-rekey-in", inputPath, "-legacy-breg", legacyPath, "-new-breg", currentPath, "-fallback-rekey-out", outputPath}
	withForeign := func(foreign ...string) []string {
		return append(append([]string(nil), rekeyArgs...), foreign...)
	}
	tests := []struct {
		name       string
		args       []string
		want       int
		wantReport bool
	}{
		{name: "complete mode", args: rekeyArgs, want: 0, wantReport: true},
		{name: "optional manifest accepted", args: withForeign("-fallback-rekey-manifest", filepath.Join(dir, "manifest.json")), want: 0},
		{name: "missing required member", args: rekeyArgs[:6], want: 2},
		{name: "lone optional flag", args: []string{"-fallback-rekey-manifest", filepath.Join(dir, "manifest.json")}, want: 2},
		{name: "foreign block-mode output", args: withForeign("-out", filepath.Join(dir, "foreign.bin")), want: 2},
		{name: "foreign filter-mode input", args: withForeign("-fallback-in", inputPath), want: 2},
		{name: "foreign refresh-bindings switch", args: withForeign("-refresh-bindings"), want: 2},
		{name: "foreign physics-mode flag", args: withForeign("-physics-breg", legacyPath), want: 2},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			command := exec.Command(binary, test.args...)
			output, err := command.CombinedOutput()
			if got := commandExitCode(err); got != test.want {
				t.Fatalf("exit = %d, want %d; output=%s", got, test.want, output)
			}
			hasReport := strings.Contains(string(output), "\"entries_joined\"")
			if test.wantReport && !hasReport {
				t.Fatalf("no-manifest success did not echo the stats summary: %s", output)
			}
			if !test.wantReport && hasReport {
				t.Fatalf("unexpected stdout stats summary: %s", output)
			}
		})
		if test.want != 0 {
			continue
		}
		if _, err := os.Stat(outputPath); err != nil {
			t.Fatalf("%s did not write the output: %v", test.name, err)
		}
	}
}
