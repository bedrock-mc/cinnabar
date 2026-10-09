package experience

import (
	"bytes"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"os"
	"path/filepath"
	"reflect"
	"runtime"
	"slices"
	"strconv"
	"strings"
	"sync"
	"testing"
	"time"
)

// startProbe supervises the real runtime on the shared probe artifact. Close must succeed at
// cleanup.
func startProbe(t *testing.T, log *slog.Logger) (*Supervisor, Loaded) {
	t.Helper()
	return startRuntime(t, probeDir, log)
}

// startRuntime supervises the real runtime on the artifact in dir. Close must succeed at cleanup.
func startRuntime(t *testing.T, dir string, log *slog.Logger) (*Supervisor, Loaded) {
	t.Helper()
	s, loaded, err := StartSupervisor(runtimeBinary, dir, log)
	if err != nil {
		t.Fatalf("StartSupervisor: %v", err)
	}
	t.Cleanup(func() {
		if err := s.Close(); err != nil {
			t.Errorf("Close: %v", err)
		}
	})
	return s, loaded
}

// fakeLoaded is the fake helper's answer to load unless a test scripts another. Each call
// returns a value of its own, which the test may change.
func fakeLoaded() Loaded {
	return Loaded{
		Protocol: protocolVersion,
		ID:       "fake",
		Version:  "0.1.0",
		Blocks: []BlockDef{
			{
				ID:          "fake:cube",
				DisplayName: "Fake Cube",
				Textures:    []Texture{{Slot: "*", Path: "/fake/assets/cube.png"}},
				Mining:      Mining{Breakable: &Breakable{Hardness: 1.5}},
			},
			{
				ID:          "fake:wall",
				DisplayName: "Fake Wall",
				Textures: []Texture{
					{Slot: "*", Path: "/fake/assets/wall.png"},
					{Slot: "up", Path: "/fake/assets/wall_top.png"},
				},
				Mining: Mining{Unbreakable: &Unbreakable{}},
			},
		},
	}
}

// fakeEnv scripts the fake helper: its mode, its answer to load, and the frame limit that its
// oversized answer exceeds.
func fakeEnv(t *testing.T, mode string, loaded Loaded) []string {
	t.Helper()
	answer, err := json.Marshal(Response{Loaded: &loaded})
	if err != nil {
		t.Fatalf("encoding the fake's answer to load: %v", err)
	}
	return []string{
		"FAKE_MODE=" + mode,
		"FAKE_LOADED=" + string(answer),
		"FAKE_MAX_FRAME=" + strconv.Itoa(maxFrameBytes),
	}
}

// startFake supervises the fake helper in mode, answering load with fakeLoaded. It is closed at
// cleanup, where a fake that ignores shutdown may make Close fail; then every helper that the
// supervisor started must have been reaped.
func startFake(t *testing.T, mode string, log *slog.Logger, opts startOptions) (*Supervisor, Loaded) {
	t.Helper()
	opts.env = fakeEnv(t, mode, fakeLoaded())
	// Called with the supervisor's lock held, or before StartSupervisor returns; Close orders
	// every call before the cleanup's read.
	var spawned []*helper
	opts.spawned = func(h *helper) { spawned = append(spawned, h) }
	s, loaded, err := startSupervisor(fakeHelperBinary, t.TempDir(), log, opts)
	if err != nil {
		t.Fatalf("starting the fake helper in mode %s: %v", mode, err)
	}
	t.Cleanup(func() {
		s.Close()
		for i, h := range spawned {
			select {
			case <-h.exited:
			default:
				t.Errorf("helper %d of %d outlived Close", i+1, len(spawned))
			}
		}
	})
	return s, loaded
}

// rescript makes every helper that s starts from now on run the fake in mode, answering load
// with loaded.
func rescript(t *testing.T, s *Supervisor, mode string, loaded Loaded) {
	t.Helper()
	env := fakeEnv(t, mode, loaded)
	s.mu.Lock()
	defer s.mu.Unlock()
	s.opts.env = env
}

// currentHelper is the helper process that s runs, or nil.
func currentHelper(s *Supervisor) *helper {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.proc
}

// assertReaped checks that the process of h has exited and been waited for.
func assertReaped(t *testing.T, h *helper) {
	t.Helper()
	select {
	case <-h.exited:
	default:
		t.Fatal("the helper process was not reaped")
	}
}

// assertRestarted checks that old was reaped and that a new helper runs in its place.
func assertRestarted(t *testing.T, s *Supervisor, old *helper) {
	t.Helper()
	assertReaped(t, old)
	if current := currentHelper(s); current == nil || current == old {
		t.Fatal("the helper was not restarted")
	}
}

// fakeClock is a settable clock for the strike and restart windows.
type fakeClock struct {
	mu  sync.Mutex
	now time.Time
}

func newFakeClock() *fakeClock {
	return &fakeClock{now: time.Date(2026, 10, 1, 12, 0, 0, 0, time.UTC)}
}

func (c *fakeClock) Now() time.Time {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.now
}

func (c *fakeClock) Advance(d time.Duration) {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.now = c.now.Add(d)
}

// restartStep is a spacing of faults that puts each in a strike window of its own while
// maxRestarts+1 of them still fall in one restart window.
func restartStep(t *testing.T) time.Duration {
	t.Helper()
	step := strikeWindow + time.Second
	if maxRestarts*step >= restartWindow {
		t.Fatalf("%d faults %v apart do not fit in the %v restart window", maxRestarts+1, step, restartWindow)
	}
	return step
}

// The real runtime loads the probe, and an interaction with x=0 commits its data write and its
// tell.
func TestProbeCallbackRoundTrip(t *testing.T) {
	log, _ := testLog(t)
	s, loaded := startProbe(t, log)
	if loaded.Protocol != protocolVersion {
		t.Fatalf("loaded protocol %d, want %d", loaded.Protocol, protocolVersion)
	}
	if len(loaded.Blocks) != 3 || loaded.Blocks[0].ID != probeCounter || loaded.Blocks[1].ID != probeLamp ||
		loaded.Blocks[2].ID != probeNode || !loaded.Blocks[2].Network {
		t.Fatalf("loaded blocks %s, want %s, %s and the network member %s", jsonOf(loaded.Blocks),
			probeCounter, probeLamp, probeNode)
	}
	got, err := s.Call(probeInteract(probeCount))
	if err != nil {
		t.Fatalf("Call: %v", err)
	}
	count := hex.EncodeToString(binary.LittleEndian.AppendUint32(nil, 1))
	want := Outcome{Committed: &Committed{Ops: []Op{
		{SetBlockData: &SetBlockDataOp{Pos: probePos(probeCount), Data: &count}},
		{Tell: &TellOp{Player: probeActor, Text: "count 1"}},
	}}}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("outcome %s, want %s", jsonOf(got), jsonOf(want))
	}
}

// A helper that never answers is killed at the result deadline, and a new one replaces it.
func TestHangIsKilledWithinDeadline(t *testing.T) {
	log, _ := testLog(t)
	s, _ := startFake(t, "hang", log, startOptions{})
	old := currentHelper(s)
	start := time.Now()
	_, err := s.Call(probeInteract(probeCount))
	elapsed := time.Since(start)
	if !errors.Is(err, errHelperFault) {
		t.Fatalf("Call: %v, want errHelperFault", err)
	}
	if limit := resultDeadline + 500*time.Millisecond; elapsed < resultDeadline || elapsed >= limit {
		t.Fatalf("Call returned after %v, want at least %v and under %v", elapsed, resultDeadline, limit)
	}
	assertRestarted(t, s, old)
}

// assertPromptFault checks that the fake helper in mode faults the first Call without waiting
// for the result deadline, and is replaced.
func assertPromptFault(t *testing.T, mode string) {
	t.Helper()
	log, _ := testLog(t)
	s, _ := startFake(t, mode, log, startOptions{})
	old := currentHelper(s)
	start := time.Now()
	_, err := s.Call(probeInteract(probeCount))
	if !errors.Is(err, errHelperFault) {
		t.Fatalf("Call: %v, want errHelperFault", err)
	}
	if elapsed := time.Since(start); elapsed >= resultDeadline {
		t.Fatalf("the fault took %v; it must not wait for the %v deadline", elapsed, resultDeadline)
	}
	assertRestarted(t, s, old)
}

// An answer that is not JSON, or a length over the frame limit, is a fault.
func TestGarbageAndOversizedAreFaults(t *testing.T) {
	for _, mode := range []string{"garbage", "oversized"} {
		t.Run(mode, func(t *testing.T) { assertPromptFault(t, mode) })
	}
}

// A helper that exits instead of answering is a fault.
func TestExitIsFault(t *testing.T) {
	assertPromptFault(t, "exit")
}

// A result that answers another seq is a fault.
func TestWrongSeqIsFault(t *testing.T) {
	assertPromptFault(t, "wrong_seq")
}

// Rejected outcomes are guest errors, not strikes; strikeLimit failed ones within strikeWindow
// quarantine the Experience, and later calls fail without IPC.
func TestStrikesQuarantine(t *testing.T) {
	log, _ := testLog(t)
	s, _ := startProbe(t, log)
	for range strikeLimit {
		out, err := s.Call(probeInteract(probeReject))
		if err != nil || out.Rejected == nil {
			t.Fatalf("x=%d: %s, %v; want rejected", probeReject, jsonOf(out), err)
		}
	}
	if s.Quarantined() {
		t.Fatal("rejected outcomes quarantined the Experience")
	}
	for i := 1; i <= strikeLimit; i++ {
		out, err := s.Call(probeInteract(probeTrap))
		if err != nil || out.Failed == nil || out.Failed.Kind != FailTrap {
			t.Fatalf("x=%d: %s, %v; want failed with trap", probeTrap, jsonOf(out), err)
		}
		if got, want := s.Quarantined(), i == strikeLimit; got != want {
			t.Fatalf("after %d failed outcomes Quarantined() = %v, want %v", i, got, want)
		}
	}
	if _, err := s.Call(probeInteract(probeCount)); !errors.Is(err, errQuarantined) {
		t.Fatalf("Call while quarantined: %v, want errQuarantined", err)
	}
	if currentHelper(s) != nil {
		t.Fatal("a quarantined Experience still runs its helper")
	}
}

// Helper faults are strikes too.
func TestFaultsCountAsStrikes(t *testing.T) {
	log, _ := testLog(t)
	s, _ := startFake(t, "exit", log, startOptions{})
	for i := 1; i <= strikeLimit; i++ {
		if _, err := s.Call(probeInteract(probeCount)); !errors.Is(err, errHelperFault) {
			t.Fatalf("fault %d: %v, want errHelperFault", i, err)
		}
		if got, want := s.Quarantined(), i == strikeLimit; got != want {
			t.Fatalf("after %d faults Quarantined() = %v, want %v", i, got, want)
		}
	}
	if _, err := s.Call(probeInteract(probeCount)); !errors.Is(err, errQuarantined) {
		t.Fatalf("Call while quarantined: %v, want errQuarantined", err)
	}
}

// A restart whose load fails is a fault, and so is one whose artifact no longer has the id and
// the blocks that were registered at startup: a strike, with the helper killed and the restart
// retried by the next Call.
func TestFailedRestartIsFault(t *testing.T) {
	for _, tc := range []struct {
		name   string
		mode   string
		change func(*Loaded)
	}{
		{"load failed", "load_failed", func(*Loaded) {}},
		{"id", "ok", func(l *Loaded) { l.ID = "other" }},
		{"block added", "ok", func(l *Loaded) { l.Blocks = append(l.Blocks, fakeLoaded().Blocks[0]) }},
		{"block removed", "ok", func(l *Loaded) { l.Blocks = l.Blocks[:1] }},
		{"block id", "ok", func(l *Loaded) { l.Blocks[1].ID = "fake:door" }},
		{"display name", "ok", func(l *Loaded) { l.Blocks[0].DisplayName = "Renamed Cube" }},
		{"texture slot", "ok", func(l *Loaded) { l.Blocks[1].Textures[1].Slot = "down" }},
		{"texture path", "ok", func(l *Loaded) { l.Blocks[0].Textures[0].Path = "/fake/assets/other.png" }},
		{"texture removed", "ok", func(l *Loaded) { l.Blocks[1].Textures = l.Blocks[1].Textures[:1] }},
		{"mining", "ok", func(l *Loaded) { l.Blocks[0].Mining = Mining{Unbreakable: &Unbreakable{}} }},
		{"hardness", "ok", func(l *Loaded) { l.Blocks[0].Mining.Breakable.Hardness = 2 }},
		{"focus", "ok", func(l *Loaded) { l.Focus = !l.Focus }},
	} {
		t.Run(tc.name, func(t *testing.T) {
			log, _ := testLog(t)
			s, _ := startFake(t, "exit", log, startOptions{})
			changed := fakeLoaded()
			tc.change(&changed)
			rescript(t, s, tc.mode, changed)
			if _, err := s.Call(probeInteract(probeCount)); !errors.Is(err, errHelperFault) {
				t.Fatalf("Call: %v, want errHelperFault", err)
			}
			if currentHelper(s) != nil {
				t.Fatal("the helper of a failed restart runs")
			}
			// The first call's exit and failed restart are two strikes; each later call retries
			// the restart, which fails again.
			calls := 1
			for !s.Quarantined() && calls < strikeLimit {
				s.Call(probeInteract(probeCount))
				calls++
			}
			if want := strikeLimit - 1; !s.Quarantined() || calls != want {
				t.Fatalf("after %d calls Quarantined() = %v; want quarantined after %d", calls, s.Quarantined(), want)
			}
		})
	}
}

// More than maxRestarts restarts within restartWindow quarantine the Experience, even when no
// strikeWindow holds strikeLimit strikes.
func TestRestartLimitQuarantines(t *testing.T) {
	step := restartStep(t)
	clock := newFakeClock()
	log, _ := testLog(t)
	s, _ := startFake(t, "exit", log, startOptions{now: clock.Now})
	for i := 1; i <= maxRestarts+1; i++ {
		if _, err := s.Call(probeInteract(probeCount)); !errors.Is(err, errHelperFault) {
			t.Fatalf("fault %d: %v, want errHelperFault", i, err)
		}
		if got, want := s.Quarantined(), i > maxRestarts; got != want {
			t.Fatalf("after %d restarts Quarantined() = %v, want %v", i, got, want)
		}
		clock.Advance(step)
	}
	if _, err := s.Call(probeInteract(probeCount)); !errors.Is(err, errQuarantined) {
		t.Fatalf("Call while quarantined: %v, want errQuarantined", err)
	}
}

// Reload lifts the quarantine with a fresh helper and forgets the strikes.
func TestReloadClearsQuarantine(t *testing.T) {
	log, _ := testLog(t)
	s, _ := startProbe(t, log)
	for range strikeLimit {
		s.Call(probeInteract(probeTrap))
	}
	if !s.Quarantined() {
		t.Fatal("not quarantined")
	}
	if err := s.Reload(); err != nil {
		t.Fatalf("Reload: %v", err)
	}
	if s.Quarantined() {
		t.Fatal("still quarantined after Reload")
	}
	if out, err := s.Call(probeInteract(probeCount)); err != nil || out.Committed == nil {
		t.Fatalf("Call after Reload: %s, %v; want committed", jsonOf(out), err)
	}
	for range strikeLimit - 1 {
		s.Call(probeInteract(probeTrap))
	}
	if s.Quarantined() {
		t.Fatal("strikes from before Reload still count")
	}
}

// Reload forgets the restarts too.
func TestReloadClearsRestartHistory(t *testing.T) {
	step := restartStep(t)
	clock := newFakeClock()
	log, _ := testLog(t)
	s, _ := startFake(t, "exit", log, startOptions{now: clock.Now})
	faults := func() {
		t.Helper()
		for range maxRestarts {
			if _, err := s.Call(probeInteract(probeCount)); !errors.Is(err, errHelperFault) {
				t.Fatalf("Call: %v, want errHelperFault", err)
			}
			clock.Advance(step)
		}
	}
	faults()
	if err := s.Reload(); err != nil {
		t.Fatalf("Reload: %v", err)
	}
	faults()
	if s.Quarantined() {
		t.Fatal("restarts from before Reload still count")
	}
}

// A Reload whose artifact no longer has the id and the blocks registered at startup fails,
// naming the difference, and leaves the Experience quarantined without a helper, whether it was
// quarantined before or not.
func TestReloadRefusesChangedArtifact(t *testing.T) {
	refused := func(t *testing.T, s *Supervisor, difference string) {
		t.Helper()
		err := s.Reload()
		if err == nil || !strings.Contains(err.Error(), difference) {
			t.Fatalf("Reload: %v; want an error naming %s", err, difference)
		}
		if !s.Quarantined() {
			t.Fatal("not quarantined after the refused Reload")
		}
		if currentHelper(s) != nil {
			t.Fatal("a helper runs after the refused Reload")
		}
		if _, err := s.Call(probeInteract(probeCount)); !errors.Is(err, errQuarantined) {
			t.Fatalf("Call after the refused Reload: %v, want errQuarantined", err)
		}
	}
	t.Run("quarantined", func(t *testing.T) {
		log, _ := testLog(t)
		s, _ := startFake(t, "exit", log, startOptions{})
		for range strikeLimit {
			s.Call(probeInteract(probeCount))
		}
		if !s.Quarantined() {
			t.Fatal("not quarantined")
		}
		changed := fakeLoaded()
		changed.ID = "changed_id"
		rescript(t, s, "ok", changed)
		refused(t, s, changed.ID)
	})
	t.Run("healthy", func(t *testing.T) {
		log, _ := testLog(t)
		s, _ := startFake(t, "ok", log, startOptions{})
		changed := fakeLoaded()
		changed.Blocks[0].DisplayName = "Changed Cube Name"
		rescript(t, s, "ok", changed)
		refused(t, s, changed.Blocks[0].DisplayName)
	})
}

// Reload may load a new version of the artifact, since its id and blocks are the registered
// ones.
func TestReloadAcceptsNewVersion(t *testing.T) {
	dir := freshProbe(t)
	log, _ := testLog(t)
	s, loaded := startRuntime(t, dir, log)
	for range strikeLimit {
		s.Call(probeInteract(probeTrap))
	}
	if !s.Quarantined() {
		t.Fatal("not quarantined")
	}
	path := filepath.Join(dir, manifestFile)
	manifest, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	was := fmt.Sprintf("version = %q", loaded.Version)
	now := fmt.Sprintf("version = %q", loaded.Version+"-fixed")
	if !bytes.Contains(manifest, []byte(was)) {
		t.Fatalf("%s has no line %s:\n%s", manifestFile, was, manifest)
	}
	if err := os.WriteFile(path, bytes.Replace(manifest, []byte(was), []byte(now), 1), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := s.Reload(); err != nil {
		t.Fatalf("Reload: %v", err)
	}
	if s.Quarantined() {
		t.Fatal("still quarantined after Reload")
	}
	if out, err := s.Call(probeInteract(probeCount)); err != nil || out.Committed == nil {
		t.Fatalf("Call after Reload: %s, %v; want committed", jsonOf(out), err)
	}
}

// StartSupervisor fails with the runtime's reason when the load fails, and refuses a helper
// that speaks another protocol version.
func TestStartReportsLoadFailure(t *testing.T) {
	t.Run("load_failed", func(t *testing.T) {
		log, _ := testLog(t)
		opts := startOptions{env: fakeEnv(t, "load_failed", fakeLoaded())}
		_, _, err := startSupervisor(fakeHelperBinary, t.TempDir(), log, opts)
		if err == nil || !strings.Contains(err.Error(), "scripted load failure") {
			t.Fatalf("StartSupervisor: %v; want the helper's reason", err)
		}
	})
	t.Run("protocol", func(t *testing.T) {
		log, _ := testLog(t)
		loaded := fakeLoaded()
		loaded.Protocol++
		opts := startOptions{env: fakeEnv(t, "ok", loaded)}
		_, _, err := startSupervisor(fakeHelperBinary, t.TempDir(), log, opts)
		if err == nil || !strings.Contains(err.Error(), "protocol") {
			t.Fatalf("StartSupervisor: %v; want the protocol version refused", err)
		}
	})
}

// The helper gets nothing of the adapter's environment beyond what the OS needs to start it.
func TestHelperEnvironmentIsCleared(t *testing.T) {
	t.Setenv("EXPERIENCE_TEST_SECRET", "leaked")
	log, logs := testLog(t)
	s, _ := startFake(t, "env", log, startOptions{})
	if _, err := s.Call(probeInteract(probeCount)); err != nil {
		t.Fatalf("Call: %v", err)
	}
	record := waitForRecord(t, logs, func(record map[string]any) bool {
		line, _ := record["line"].(string)
		return strings.HasPrefix(line, "env ")
	})
	var got []string
	if err := json.Unmarshal([]byte(strings.TrimPrefix(record["line"].(string), "env ")), &got); err != nil {
		t.Fatal(err)
	}
	var want []string
	for _, entry := range fakeEnv(t, "env", fakeLoaded()) {
		name, _, _ := strings.Cut(entry, "=")
		want = append(want, name)
	}
	if runtime.GOOS == "windows" {
		want = append(want, "SystemRoot")
	}
	slices.Sort(got)
	slices.Sort(want)
	if !slices.Equal(got, want) {
		t.Fatalf("helper environment %q, want %q", got, want)
	}
}

// Each line the helper writes to stderr is logged with the Experience id.
func TestHelperStderrIsLogged(t *testing.T) {
	log, logs := testLog(t)
	s, loaded := startFake(t, "ok", log, startOptions{})
	if _, err := s.Call(probeInteract(probeCount)); err != nil {
		t.Fatalf("Call: %v", err)
	}
	waitForRecord(t, logs, func(record map[string]any) bool {
		return record["experience"] == loaded.ID && record["line"] == "callback 1"
	})
}

// Close kills a helper that does not exit within shutdownGrace of the shutdown frame.
func TestCloseKillsUnresponsiveHelper(t *testing.T) {
	log, _ := testLog(t)
	s, _ := startFake(t, "hang", log, startOptions{})
	old := currentHelper(s)
	start := time.Now()
	err := s.Close()
	elapsed := time.Since(start)
	if err == nil {
		t.Fatal("Close reported a clean exit for a helper that ignores shutdown")
	}
	if limit := shutdownGrace + 500*time.Millisecond; elapsed < shutdownGrace || elapsed >= limit {
		t.Fatalf("Close returned after %v, want at least %v and under %v", elapsed, shutdownGrace, limit)
	}
	assertReaped(t, old)
}
