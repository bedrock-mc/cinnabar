package main

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"slices"
	"sort"
	"strconv"
	"strings"
	"testing"

	"gopkg.in/yaml.v3"
)

func TestGenerateIsDeterministicAndPinned(t *testing.T) {
	if err := verifySourcePin(); err != nil {
		t.Fatalf("verify Dragonfly dependency pin: %v", err)
	}
	firstDir, secondDir := t.TempDir(), t.TempDir()
	if err := generate(firstDir); err != nil {
		t.Fatalf("generate first corpus: %v", err)
	}
	if err := generate(secondDir); err != nil {
		t.Fatalf("generate second corpus: %v", err)
	}

	firstManifestBytes := readFile(t, filepath.Join(firstDir, "manifest.json"))
	secondManifestBytes := readFile(t, filepath.Join(secondDir, "manifest.json"))
	if !bytes.Equal(firstManifestBytes, secondManifestBytes) {
		t.Fatal("manifest differs between identical generator runs")
	}
	if len(firstManifestBytes) == 0 || firstManifestBytes[len(firstManifestBytes)-1] != '\n' {
		t.Fatal("manifest must end in a newline")
	}

	var manifest fixtureManifest
	if err := json.Unmarshal(firstManifestBytes, &manifest); err != nil {
		t.Fatalf("decode manifest: %v", err)
	}
	if manifest.Source != (sourceDescriptor{Module: sourceModule, Version: sourceVersion, Commit: sourceCommit}) {
		t.Fatalf("source provenance = %+v", manifest.Source)
	}

	wantFiles := []string{
		"uniform_non_air.bin",
		"checkerboard.bin",
		"vertical_layers.bin",
		"two_storage_layers.bin",
		"bits_1.bin",
		"bits_2.bin",
		"bits_3.bin",
		"bits_4.bin",
		"bits_5.bin",
		"bits_6.bin",
		"bits_8.bin",
		"bits_16.bin",
	}
	if len(manifest.Fixtures) != len(wantFiles) {
		t.Fatalf("fixture count = %d, want %d", len(manifest.Fixtures), len(wantFiles))
	}
	for i, fixture := range manifest.Fixtures {
		if fixture.File != wantFiles[i] {
			t.Fatalf("fixture %d file = %q, want %q", i, fixture.File, wantFiles[i])
		}
		if fixture.Version != 9 || fixture.YIndex != -4 {
			t.Fatalf("%s header metadata = version %d/y %d", fixture.File, fixture.Version, fixture.YIndex)
		}
		first := readFile(t, filepath.Join(firstDir, fixture.File))
		second := readFile(t, filepath.Join(secondDir, fixture.File))
		if !bytes.Equal(first, second) {
			t.Fatalf("%s differs between identical generator runs", fixture.File)
		}
		version, yIndex, storages, err := inspectSubChunk(first)
		if err != nil {
			t.Fatalf("inspect %s: %v", fixture.File, err)
		}
		if version != fixture.Version || yIndex != fixture.YIndex || !slices.Equal(storages, fixture.Storages) {
			t.Fatalf(
				"%s binary metadata = version %d/y %d/%+v, manifest = %d/%d/%+v",
				fixture.File, version, yIndex, storages, fixture.Version, fixture.YIndex, fixture.Storages,
			)
		}
	}

	_, sourceFile, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("resolve test source path")
	}
	committedDir := filepath.Clean(filepath.Join(filepath.Dir(sourceFile), "..", "..", "crates", "world", "fixtures"))
	assertDirectoriesEqual(t, firstDir, committedDir)
}

func TestChunkfixIsIsolatedFromWorkspaceAndCoveredByCI(t *testing.T) {
	_, sourceFile, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("resolve test source path")
	}
	repoRoot := filepath.Clean(filepath.Join(filepath.Dir(sourceFile), "..", ".."))

	usesChunkfix, err := workspaceUsesDirectory(
		filepath.Join(repoRoot, "go.work"),
		repoRoot,
		filepath.Join("tools", "chunkfix"),
	)
	if err != nil {
		t.Fatalf("inspect go.work: %v", err)
	}
	if usesChunkfix {
		t.Fatal("tools/chunkfix must stay outside go.work so its pinned Dragonfly encoder is not upgraded by workspace MVS")
	}

	covered, err := workflowHasIsolatedChunkfixStep(
		readFile(t, filepath.Join(repoRoot, ".github", "workflows", "ci.yml")),
	)
	if err != nil {
		t.Fatalf("inspect CI workflow: %v", err)
	}
	if !covered {
		t.Fatal("CI must test and vet tools/chunkfix as an isolated module with GOWORK=off")
	}
}

func TestWorkspaceUsesDirectoryCanonicalizesUsePaths(t *testing.T) {
	repoRoot := t.TempDir()
	if err := os.MkdirAll(filepath.Join(repoRoot, "tools", "chunkfix"), 0o755); err != nil {
		t.Fatalf("create chunkfix directory: %v", err)
	}
	goWorkPath := filepath.Join(repoRoot, "go.work")
	absoluteChunkfix := filepath.ToSlash(filepath.Join(repoRoot, "tools", "chunkfix"))
	tests := []struct {
		name string
		body string
		want bool
	}{
		{name: "desired workspace", body: "go 1.26.1\nuse ./core\n", want: false},
		{name: "single line use", body: "go 1.26.1\nuse ./tools/chunkfix\n", want: true},
		{name: "quoted trailing path", body: "go 1.26.1\nuse \"./tools/chunkfix/\"\n", want: true},
		{name: "absolute path", body: "go 1.26.1\nuse " + strconv.Quote(absoluteChunkfix) + "\n", want: true},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			if err := os.WriteFile(goWorkPath, []byte(test.body), 0o644); err != nil {
				t.Fatalf("write go.work: %v", err)
			}
			got, err := workspaceUsesDirectory(goWorkPath, repoRoot, filepath.Join("tools", "chunkfix"))
			if err != nil {
				t.Fatalf("inspect go.work: %v", err)
			}
			if got != test.want {
				t.Fatalf("workspaceUsesDirectory() = %t, want %t", got, test.want)
			}
		})
	}
}

func TestWorkspaceUsesDirectoryResolvesFilesystemAliases(t *testing.T) {
	repoRoot := t.TempDir()
	chunkfix := filepath.Join(repoRoot, "tools", "chunkfix")
	if err := os.MkdirAll(chunkfix, 0o755); err != nil {
		t.Fatalf("create chunkfix directory: %v", err)
	}
	alias := filepath.Join(repoRoot, "chunkfix-alias")
	symlinkErr := os.Symlink(chunkfix, alias)
	if symlinkErr != nil && runtime.GOOS == "windows" {
		output, junctionErr := exec.Command("cmd", "/c", "mklink", "/J", alias, chunkfix).CombinedOutput()
		if junctionErr != nil {
			t.Skipf(
				"filesystem aliases are unsupported in this environment: symlink: %v; junction: %v: %s",
				symlinkErr,
				junctionErr,
				strings.TrimSpace(string(output)),
			)
		}
	} else if symlinkErr != nil {
		t.Skipf("filesystem aliases are unsupported in this environment: %v", symlinkErr)
	}
	goWorkPath := filepath.Join(repoRoot, "go.work")
	if err := os.WriteFile(goWorkPath, []byte("go 1.26.1\nuse ./chunkfix-alias\n"), 0o644); err != nil {
		t.Fatalf("write go.work: %v", err)
	}
	got, err := workspaceUsesDirectory(goWorkPath, repoRoot, filepath.Join("tools", "chunkfix"))
	if err != nil {
		t.Fatalf("inspect go.work: %v", err)
	}
	if !got {
		t.Fatal("workspace alias resolving to tools/chunkfix was not detected")
	}
}

func TestWorkflowHasIsolatedChunkfixStepRequiresActiveVerifyStepStructure(t *testing.T) {
	valid := `name: CI
jobs:
  verify:
    steps:
      - run: |
          go vet ./...
          go test ./... -count=1
        env:
          GOWORK: off
        working-directory: tools/chunkfix
`
	tests := []struct {
		name string
		body string
		want bool
	}{
		{name: "harmless key ordering", body: valid, want: true},
		{name: "wrong working directory", body: strings.Replace(valid, "working-directory: tools/chunkfix", "working-directory: tools/registrygen", 1), want: false},
		{name: "missing working directory", body: strings.Replace(valid, "        working-directory: tools/chunkfix\n", "", 1), want: false},
		{name: "wrong GOWORK", body: strings.Replace(valid, "GOWORK: off", "GOWORK: on", 1), want: false},
		{name: "missing GOWORK", body: strings.Replace(valid, "        env:\n          GOWORK: off\n", "", 1), want: false},
		{name: "missing test", body: strings.Replace(valid, "          go test ./... -count=1\n", "", 1), want: false},
		{name: "missing vet", body: strings.Replace(valid, "          go vet ./...\n", "", 1), want: false},
		{
			name: "comments and unrelated literals",
			body: `name: go test ./... -count=1 and go vet ./...
jobs:
  verify:
    steps:
      - working-directory: tools/chunkfix
        env:
          GOWORK: off
        run: |
          # go test ./... -count=1
          echo "go vet ./..."
`,
			want: false,
		},
		{
			name: "heredoc literal body",
			body: `jobs:
  verify:
    steps:
      - working-directory: tools/chunkfix
        env:
          GOWORK: off
        run: |
          cat <<'CHUNKFIX_COMMANDS'
          go test ./... -count=1
          go vet ./...
          CHUNKFIX_COMMANDS
`,
			want: false,
		},
		{
			name: "quoted multiline literal",
			body: `jobs:
  verify:
    steps:
      - working-directory: tools/chunkfix
        env:
          GOWORK: off
        run: |
          printf '%s\n' '
          go test ./... -count=1
          go vet ./...
          '
`,
			want: false,
		},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			got, err := workflowHasIsolatedChunkfixStep([]byte(test.body))
			if err != nil {
				t.Fatalf("inspect workflow: %v", err)
			}
			if got != test.want {
				t.Fatalf("workflowHasIsolatedChunkfixStep() = %t, want %t", got, test.want)
			}
		})
	}
}

type goWorkDocument struct {
	Use []struct {
		DiskPath string
	}
}

func workspaceUsesDirectory(goWorkPath, repoRoot, target string) (bool, error) {
	cmd := exec.Command("go", "work", "edit", "-json", goWorkPath)
	cmd.Dir = repoRoot
	output, err := cmd.CombinedOutput()
	if err != nil {
		return false, fmt.Errorf("parse %s: %w: %s", goWorkPath, err, strings.TrimSpace(string(output)))
	}
	var document goWorkDocument
	if err := json.Unmarshal(output, &document); err != nil {
		return false, fmt.Errorf("decode go.work JSON: %w", err)
	}
	targetPath, err := canonicalDirectory(repoRoot, target)
	if err != nil {
		return false, fmt.Errorf("canonicalize target directory: %w", err)
	}
	for _, use := range document.Use {
		usePath, err := canonicalDirectory(filepath.Dir(goWorkPath), use.DiskPath)
		if err != nil {
			return false, fmt.Errorf("canonicalize use directory %q: %w", use.DiskPath, err)
		}
		if sameDirectory(usePath, targetPath) {
			return true, nil
		}
	}
	return false, nil
}

func canonicalDirectory(base, path string) (string, error) {
	if !filepath.IsAbs(path) {
		path = filepath.Join(base, path)
	}
	absolute, err := filepath.Abs(path)
	if err != nil {
		return "", err
	}
	resolved, err := filepath.EvalSymlinks(absolute)
	if err == nil {
		absolute = resolved
	} else if !errors.Is(err, os.ErrNotExist) {
		return "", err
	}
	return filepath.Clean(absolute), nil
}

func sameDirectory(left, right string) bool {
	leftInfo, leftErr := os.Stat(left)
	rightInfo, rightErr := os.Stat(right)
	if leftErr == nil && rightErr == nil && os.SameFile(leftInfo, rightInfo) {
		return true
	}
	if runtime.GOOS == "windows" {
		return strings.EqualFold(left, right)
	}
	return left == right
}

type workflowDocument struct {
	Jobs map[string]workflowJob `yaml:"jobs"`
}

type workflowJob struct {
	Steps []workflowStep `yaml:"steps"`
}

type workflowStep struct {
	WorkingDirectory string            `yaml:"working-directory"`
	Env              map[string]string `yaml:"env"`
	Run              string            `yaml:"run"`
}

func workflowHasIsolatedChunkfixStep(data []byte) (bool, error) {
	var workflow workflowDocument
	if err := yaml.Unmarshal(data, &workflow); err != nil {
		return false, fmt.Errorf("decode workflow YAML: %w", err)
	}
	verify, ok := workflow.Jobs["verify"]
	if !ok {
		return false, nil
	}
	for _, step := range verify.Steps {
		if step.WorkingDirectory != "tools/chunkfix" || step.Env["GOWORK"] != "off" {
			continue
		}
		hasTest, hasVet := chunkfixCommands(step.Run)
		if hasTest && hasVet {
			return true, nil
		}
	}
	return false, nil
}

type shellLiteralState struct {
	quote            byte
	heredocDelimiter string
	stripHeredocTabs bool
}

func chunkfixCommands(script string) (hasTest, hasVet bool) {
	state := shellLiteralState{}
	for _, line := range strings.Split(strings.ReplaceAll(script, "\r\n", "\n"), "\n") {
		if state.heredocDelimiter != "" {
			candidate := line
			if state.stripHeredocTabs {
				candidate = strings.TrimLeft(candidate, "\t")
			}
			if candidate == state.heredocDelimiter {
				state.heredocDelimiter = ""
				state.stripHeredocTabs = false
			}
			continue
		}

		startedInQuote := state.quote != 0
		if delimiter, stripTabs, ok := scanShellLiterals(line, &state.quote); ok {
			state.heredocDelimiter = delimiter
			state.stripHeredocTabs = stripTabs
			continue
		}
		if startedInQuote || state.quote != 0 {
			continue
		}
		switch strings.TrimSpace(line) {
		case "go test ./... -count=1":
			hasTest = true
		case "go vet ./...":
			hasVet = true
		}
	}
	return hasTest, hasVet
}

func scanShellLiterals(line string, quote *byte) (delimiter string, stripTabs, ok bool) {
	for i := 0; i < len(line); i++ {
		character := line[i]
		if *quote == '\'' {
			if character == '\'' {
				*quote = 0
			}
			continue
		}
		if *quote == '"' {
			if character == '\\' && i+1 < len(line) {
				i++
				continue
			}
			if character == '"' {
				*quote = 0
			}
			continue
		}
		switch character {
		case '\\':
			if i+1 < len(line) {
				i++
			}
		case '\'', '"':
			*quote = character
		case '#':
			if i == 0 || line[i-1] == ' ' || line[i-1] == '\t' {
				return "", false, false
			}
		case '<':
			if i+1 >= len(line) || line[i+1] != '<' || i+2 < len(line) && line[i+2] == '<' {
				continue
			}
			i += 2
			if i < len(line) && line[i] == '-' {
				stripTabs = true
				i++
			}
			for i < len(line) && (line[i] == ' ' || line[i] == '\t') {
				i++
			}
			if i >= len(line) {
				return "", false, false
			}
			if line[i] == '\'' || line[i] == '"' {
				mark := line[i]
				start := i + 1
				end := strings.IndexByte(line[start:], mark)
				if end == -1 {
					return "", false, false
				}
				return line[start : start+end], stripTabs, true
			}
			start := i
			for i < len(line) && !strings.ContainsRune(" \t;|&<>#", rune(line[i])) {
				i++
			}
			if i == start {
				return "", false, false
			}
			return line[start:i], stripTabs, true
		}
	}
	return "", false, false
}

func TestWidthFixturesUseMinimalPaletteBoundaries(t *testing.T) {
	want := []storageDescriptor{
		{BitsPerIndex: 1, PaletteLen: 2},
		{BitsPerIndex: 2, PaletteLen: 3},
		{BitsPerIndex: 3, PaletteLen: 5},
		{BitsPerIndex: 4, PaletteLen: 9},
		{BitsPerIndex: 5, PaletteLen: 17},
		{BitsPerIndex: 6, PaletteLen: 33},
		{BitsPerIndex: 8, PaletteLen: 65},
		{BitsPerIndex: 16, PaletteLen: 257},
	}
	specs := fixtureSpecs()
	if len(specs) != 4+len(want) {
		t.Fatalf("spec count = %d, want %d", len(specs), 4+len(want))
	}
	for i, descriptor := range want {
		got := specs[4+i].storages
		if len(got) != 1 || got[0] != descriptor {
			t.Fatalf("width spec %d = %+v, want %+v", i, got, descriptor)
		}
	}
}

func readFile(t *testing.T, path string) []byte {
	t.Helper()
	b, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read %s: %v", path, err)
	}
	return b
}

func assertDirectoriesEqual(t *testing.T, generatedDir, committedDir string) {
	t.Helper()
	generated := directoryFiles(t, generatedDir)
	committed := directoryFiles(t, committedDir)
	if !slices.Equal(generated, committed) {
		t.Fatalf("fixture file set drift: generated %v, committed %v", generated, committed)
	}
	for _, name := range generated {
		if !bytes.Equal(readFile(t, filepath.Join(generatedDir, name)), readFile(t, filepath.Join(committedDir, name))) {
			t.Fatalf("committed fixture %s differs from freshly generated output", name)
		}
	}
}

func directoryFiles(t *testing.T, dir string) []string {
	t.Helper()
	entries, err := os.ReadDir(dir)
	if err != nil {
		t.Fatalf("read fixture directory %s: %v", dir, err)
	}
	files := make([]string, 0, len(entries))
	for _, entry := range entries {
		if entry.IsDir() {
			t.Fatalf("unexpected directory in fixture corpus: %s", entry.Name())
		}
		files = append(files, entry.Name())
	}
	sort.Strings(files)
	return files
}
