package experience

import (
	"bufio"
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"image"
	"image/color"
	"image/png"
	"log/slog"
	"maps"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"
)

// Set by TestMain.
var (
	// runtimeBinary is the experience-runtime executable.
	runtimeBinary string
	// fakeHelperBinary is testdata/fakehelper, a stand-in helper scripted by FAKE_MODE.
	fakeHelperBinary string
	// probeDir is an artifact of the probe guest. Tests only read it; freshProbe makes one to
	// change.
	probeDir string
	// repoRoot is the Cinnabar repository, and probeWasm the probe guest's core module.
	repoRoot, probeWasm string
)

// The artifact layout the runtime expects.
const (
	manifestFile = "experience.toml"
	serverWasm   = "server.wasm"
	counterPNG   = "assets/counter.png"
)

// TestMain builds experience-runtime and the probe guest with cargo, assembles the probe
// artifact and builds the fake helper. It fails when cargo is missing.
func TestMain(m *testing.M) {
	scratch, err := os.MkdirTemp("", "experience-test-")
	if err != nil {
		fmt.Fprintln(os.Stderr, "experience tests:", err)
		os.Exit(1)
	}
	if err := buildHelpers(scratch); err != nil {
		fmt.Fprintln(os.Stderr, "experience tests:", err)
		os.RemoveAll(scratch)
		os.Exit(1)
	}
	code := m.Run()
	os.RemoveAll(scratch)
	os.Exit(code)
}

// buildHelpers sets every variable that TestMain sets. Everything it writes goes below scratch,
// except what cargo keeps in its target directories.
func buildHelpers(scratch string) error {
	if _, err := exec.LookPath("cargo"); err != nil {
		return fmt.Errorf("cargo is required to build experience-runtime and the probe guest: %w", err)
	}
	// The package lives in tools/localserver/experience.
	root, err := filepath.Abs(filepath.Join("..", "..", ".."))
	if err != nil {
		return err
	}
	repoRoot = root
	runtimeBinary, err = cargoArtifact(root, "experience-runtime",
		"build", "-p", "experience-runtime", "--locked")
	if err != nil {
		return err
	}
	// Like the runtime's own tests, the guest gets a target directory of its own.
	probeWasm, err = cargoArtifact(root, "experience_probe",
		"build", "--locked", "--target", "wasm32-unknown-unknown", "-p", "experience-probe",
		"--target-dir", filepath.Join(root, "target", "experience-guests"))
	if err != nil {
		return err
	}
	probeDir = filepath.Join(scratch, "probe")
	if err := assembleProbe(root, probeWasm, probeDir); err != nil {
		return fmt.Errorf("assembling the probe artifact: %w", err)
	}
	fakeHelperBinary = filepath.Join(scratch, "fakehelper")
	if runtime.GOOS == "windows" {
		fakeHelperBinary += ".exe"
	}
	build := exec.Command("go", "build", "-o", fakeHelperBinary, "./testdata/fakehelper")
	if out, err := build.CombinedOutput(); err != nil {
		return fmt.Errorf("building the fake helper: %v\n%s", err, out)
	}
	return nil
}

// cargoArtifact runs cargo with args in root and returns the file it built for the target
// named target: the executable, or else the .wasm.
func cargoArtifact(root, target string, args ...string) (string, error) {
	cmd := exec.Command("cargo", append(args, "--message-format=json-render-diagnostics")...)
	cmd.Dir = root
	var stderr bytes.Buffer
	cmd.Stderr = &stderr
	out, err := cmd.Output()
	if err != nil {
		return "", fmt.Errorf("cargo %s: %v\n%s", strings.Join(args, " "), err, stderr.Bytes())
	}
	file := ""
	messages := json.NewDecoder(bytes.NewReader(out))
	for messages.More() {
		var msg struct {
			Reason string `json:"reason"`
			Target struct {
				Name string `json:"name"`
			} `json:"target"`
			Filenames  []string `json:"filenames"`
			Executable string   `json:"executable"`
		}
		if err := messages.Decode(&msg); err != nil {
			return "", fmt.Errorf("cargo %s: reading its messages: %w", strings.Join(args, " "), err)
		}
		if msg.Reason != "compiler-artifact" || msg.Target.Name != target {
			continue
		}
		file = msg.Executable
		if i := slices.IndexFunc(msg.Filenames, func(f string) bool {
			return strings.HasSuffix(f, ".wasm")
		}); file == "" && i >= 0 {
			file = msg.Filenames[i]
		}
	}
	if file == "" {
		return "", fmt.Errorf("cargo %s built nothing for %s", strings.Join(args, " "), target)
	}
	return file, nil
}

// assembleProbe writes the probe artifact into dir like the runtime tests' probe_dir: the
// probe's experience.toml with [files] holding the SHA-256 of every other file, server.wasm,
// and a 1×1 opaque PNG as assets/counter.png.
func assembleProbe(root, wasm, dir string) error {
	manifest, err := os.ReadFile(filepath.Join(root, "examples", "experiences", "probe", manifestFile))
	if err != nil {
		return err
	}
	module, err := os.ReadFile(wasm)
	if err != nil {
		return err
	}
	img := image.NewNRGBA(image.Rect(0, 0, 1, 1))
	img.Set(0, 0, color.NRGBA{R: 0x40, G: 0x80, B: 0xc0, A: 0xff})
	var texture bytes.Buffer
	if err := png.Encode(&texture, img); err != nil {
		return err
	}
	files := map[string][]byte{serverWasm: module, counterPNG: texture.Bytes()}

	index := strings.TrimRight(string(manifest), "\r\n")
	if !strings.HasSuffix(index, "[files]") {
		return fmt.Errorf("the probe's %s no longer ends with its empty [files] table", manifestFile)
	}
	index += "\n"
	for _, name := range slices.Sorted(maps.Keys(files)) {
		path := filepath.Join(dir, filepath.FromSlash(name))
		if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
			return err
		}
		if err := os.WriteFile(path, files[name], 0o644); err != nil {
			return err
		}
		sum := sha256.Sum256(files[name])
		index += fmt.Sprintf("%q = %q\n", name, hex.EncodeToString(sum[:]))
	}
	return os.WriteFile(filepath.Join(dir, manifestFile), []byte(index), 0o644)
}

// freshProbe assembles a probe artifact of the test's own, which it may change.
func freshProbe(t *testing.T) string {
	t.Helper()
	dir := t.TempDir()
	if err := assembleProbe(repoRoot, probeWasm, dir); err != nil {
		t.Fatalf("assembling a probe artifact: %v", err)
	}
	return dir
}

// The probe guest selects a behavior by the x of the interacted block.
const (
	// probeCount increments the little-endian u32 in the block's data and tells "count <n>".
	probeCount = 0
	// probeTrap stages a data write, a tell and a client message, then traps.
	probeTrap = 1
	// probeReject stages a tell, then returns a guest error.
	probeReject = 10
)

const (
	probeActor   = "3f2a7c1e-8b4d-4e6a-9c5f-1d2e3f4a5b6c"
	probeCounter = "probe:counter"
)

// probePos is the block that the probe behavior x runs on.
func probePos(x int32) BlockPos {
	return BlockPos{X: x, Y: 64}
}

// probeInteract is the actor's interaction with probePos(x). The snapshot holds that block, an
// owned probe counter without data, and its six neighbors, loaded air; the world height and the
// data budget leave room.
func probeInteract(x int32) CallbackRequest {
	anchor := probePos(x)
	snapshot := []Cell{{Pos: anchor, Loaded: true, ID: probeCounter, Owned: true}}
	for _, d := range []BlockPos{{X: 1}, {X: -1}, {Y: 1}, {Y: -1}, {Z: 1}, {Z: -1}} {
		pos := BlockPos{X: anchor.X + d.X, Y: anchor.Y + d.Y, Z: anchor.Z + d.Z}
		snapshot = append(snapshot, Cell{Pos: pos, Loaded: true, ID: airID})
	}
	actor := probeActor
	return CallbackRequest{
		Info:       Info{WorldID: "world", DimensionID: "overworld", Tick: 1, EventSequence: 1},
		Actor:      &actor,
		WorldMinY:  -64,
		WorldMaxY:  319,
		DataBudget: 1 << 20,
		Snapshot:   snapshot,
		Call:       Call{Interact: &InteractCall{Player: probeActor, Pos: anchor, Face: FaceUp}},
	}
}

// logBuffer collects the records of a JSON slog handler, which writes each record with one Write.
type logBuffer struct {
	mu  sync.Mutex
	buf bytes.Buffer
}

func (b *logBuffer) Write(p []byte) (int, error) {
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.buf.Write(p)
}

func (b *logBuffer) String() string {
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.buf.String()
}

// records returns every record logged so far.
func (b *logBuffer) records(t *testing.T) []map[string]any {
	t.Helper()
	var records []map[string]any
	lines := bufio.NewScanner(strings.NewReader(b.String()))
	for lines.Scan() {
		var record map[string]any
		if err := json.Unmarshal(lines.Bytes(), &record); err != nil {
			t.Fatalf("log line %q: %v", lines.Text(), err)
		}
		records = append(records, record)
	}
	return records
}

// testLog returns a logger into a logBuffer whose records are printed if the test fails.
func testLog(t *testing.T) (*slog.Logger, *logBuffer) {
	logs := &logBuffer{}
	t.Cleanup(func() {
		if t.Failed() {
			t.Logf("log:\n%s", logs)
		}
	})
	return slog.New(slog.NewJSONHandler(logs, &slog.HandlerOptions{Level: slog.LevelDebug})), logs
}

// waitForRecord waits for a record that match accepts.
func waitForRecord(t *testing.T, logs *logBuffer, match func(record map[string]any) bool) map[string]any {
	t.Helper()
	deadline := time.Now().Add(5 * time.Second)
	for {
		for _, record := range logs.records(t) {
			if match(record) {
				return record
			}
		}
		if time.Now().After(deadline) {
			t.Fatal("no matching log record within 5s")
		}
		time.Sleep(10 * time.Millisecond)
	}
}

// jsonOf renders v for a failure message.
func jsonOf(v any) string {
	out, err := json.Marshal(v)
	if err != nil {
		return fmt.Sprintf("%+v (%v)", v, err)
	}
	return string(out)
}
