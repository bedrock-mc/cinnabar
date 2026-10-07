package localworld

import (
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"testing"
	"time"
)

func fakeDockerEnv(t *testing.T, extra ...string) (env []string, logPath string) {
	t.Helper()
	logPath = filepath.Join(t.TempDir(), "docker.log")
	return append([]string{helperEnv + "=docker", dockerLogEnv + "=" + logPath}, extra...), logPath
}

const testImage = "itzg/minecraft-bedrock-server:2026.9.2@sha256:0000000000000000000000000000000000000000000000000000000000000000"

// macProvisioner is a container-runtime provisioner with the Linux build already downloaded.
func macProvisioner(t *testing.T) *Provisioner {
	t.Helper()
	p := &Provisioner{Root: filepath.Join(t.TempDir(), "bds"), goos: "darwin", goarch: "arm64", Version: "1.26.52.3"}
	p.SetRuntime(RuntimeInfo{Kind: RuntimeContainer, Reason: "container"})
	dir := filepath.Join(p.Root, "1.26.52.3")
	if err := os.MkdirAll(dir, 0o700); err != nil {
		t.Fatal(err)
	}
	_ = os.WriteFile(filepath.Join(dir, "manifest.json"), []byte("{}"), 0o600)
	_ = os.WriteFile(filepath.Join(dir, "bedrock_server"), []byte("elf"), 0o700)
	return p
}

func TestContainerRunnerLifecycleAndArguments(t *testing.T) {
	env, logPath := fakeDockerEnv(t)
	p := macProvisioner(t)
	if st := p.Status(); st.State != SetupReady || st.Runtime != RuntimeContainer {
		t.Fatalf("status = %+v", st)
	}
	runner := BDSRunner{Provisioner: p, Docker: os.Args[0], Env: env, StartTimeout: 20 * time.Second, Image: testImage}
	spec := testSpec()
	spec.Dir = t.TempDir()
	if _, err := runner.Start(context.Background(), spec); err != ErrEULARequired {
		t.Fatalf("before EULA: %v", err)
	}
	if err := p.AcceptEULA(); err != nil {
		t.Fatal(err)
	}
	if st := p.Status(); st.State != SetupReady {
		t.Fatalf("status = %+v", st)
	}
	inst, err := runner.Start(context.Background(), spec)
	if err != nil {
		t.Fatal(err)
	}
	if c, ok := inst.(interface{ CanPause() bool }); !ok || c.CanPause() {
		t.Fatal("container BDS cannot pause")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	if err := inst.Stop(ctx); err != nil {
		t.Fatal(err)
	}
	raw, _ := os.ReadFile(logPath)
	log := string(raw)
	name := "cinnabar-bds-" + spec.World.ID
	for _, want := range []string{
		"info\n",
		"image inspect " + testImage,
		"pull --platform linux/amd64 " + testImage,
		"rm -f " + name,
		"run --rm --name " + name + " --platform linux/amd64 -p 127.0.0.1:",
		fmt.Sprintf(":%d/tcp", bdsContainerHTTPPort),
		"-v " + filepath.Join(p.Root, "1.26.52.3") + ":/data ",
		"-v " + filepath.Join(spec.Dir, "db") + ":/data/worlds/" + spec.World.ID,
		"-e EULA=TRUE", "-e VERSION=1.26.52.3", "-e ONLINE_MODE=false", "-e LEVEL_TYPE=FLAT", "-e LEVEL_SEED=-7", "-e ENABLE_BDS_V6BIND_FIX=TRUE", "-e TRANSPORT=" + string(TransportNetherNetHTTP), "-e ENABLE_LAN_VISIBILITY=false",
		"-e DIRECT_DOWNLOAD_URL=https://www.minecraft.net/bedrockdedicatedserver/bin-linux/bedrock-server-1.26.52.3.zip",
		"stop -t 25 " + name,
	} {
		if !strings.Contains(log, want) {
			t.Fatalf("docker log missing %q:\n%s", want, log)
		}
	}
	if _, err := os.Stat(filepath.Join(p.Root, "1.26.52.3", "bedrock_server-1.26.52.3")); err != nil {
		t.Fatalf("versioned binary missing, so the image would download its own: %v", err)
	}
	if strings.Count(log, "rm -f "+name) < 2 {
		t.Fatalf("container not removed after stop:\n%s", log)
	}
	lines := strings.Split(strings.TrimSpace(log), "\n")
	if last := lines[len(lines)-1]; last != "rm -f "+name {
		t.Fatalf("cleanup should be the last docker call, got %q", last)
	}
}

func TestContainerRunnerSkipsPullWhenImagePresent(t *testing.T) {
	env, logPath := fakeDockerEnv(t)
	_ = os.WriteFile(logPath+".pulled", nil, 0o600)
	p := macProvisioner(t)
	_ = p.AcceptEULA()
	runner := BDSRunner{Provisioner: p, Docker: os.Args[0], Env: env, StartTimeout: 20 * time.Second, Image: testImage}
	spec := testSpec()
	spec.Dir = t.TempDir()
	inst, err := runner.Start(context.Background(), spec)
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	_ = inst.Stop(ctx)
	if raw, _ := os.ReadFile(logPath); strings.Contains(string(raw), "pull ") {
		t.Fatalf("pulled an image that was present:\n%s", raw)
	}
}

func TestDetectRuntimeOrderAndUnavailableReasons(t *testing.T) {
	ctx := context.Background()
	if info := detectRuntime(ctx, "linux", "amd64", "definitely-not-docker", nil); info.Kind != RuntimeNative {
		t.Fatalf("linux = %+v", info)
	}
	if info := detectRuntime(ctx, "windows", "amd64", "definitely-not-docker", nil); info.Kind != RuntimeNative {
		t.Fatalf("windows = %+v", info)
	}
	env, _ := fakeDockerEnv(t)
	if info := detectRuntime(ctx, "darwin", "arm64", os.Args[0], env); info.Kind != RuntimeContainer || info.Unavailable != "" {
		t.Fatalf("docker up = %+v", info)
	}
	down, _ := fakeDockerEnv(t, "LOCALWORLD_TEST_DOCKER_DOWN=1")
	if info := detectRuntime(ctx, "darwin", "arm64", os.Args[0], down); info.Kind != RuntimeNone || info.Unavailable != "docker_not_running" {
		t.Fatalf("docker down = %+v", info)
	}
	if info := detectRuntime(ctx, "darwin", "arm64", "definitely-not-docker", nil); info.Kind != RuntimeNone || info.Unavailable != "docker_missing" {
		t.Fatalf("docker missing = %+v", info)
	}

}

func TestStatusExposesUnavailableReasonWithoutChangingDefaultBackend(t *testing.T) {
	store := newTestStore(t)
	store.SetDefaultBackend(BackendDragonfly)
	p := &Provisioner{Root: t.TempDir(), goos: "darwin", goarch: "arm64"}
	p.SetRuntime(RuntimeInfo{Kind: RuntimeNone, Reason: "no docker", Unavailable: "docker_not_running"})
	p.SetDetector(func(context.Context) RuntimeInfo { return RuntimeInfo{Kind: RuntimeContainer, Reason: "docker up"} })
	m := NewManager(store, Runners{}, nil)
	m.SetSetup(p)
	st := m.Status()
	if st.BackendUnavailableReason != "docker_not_running" || st.Setup == nil || st.Setup.State != SetupUnsupported {
		t.Fatalf("status = %+v", st)
	}
	if _, err := m.Prefs(context.Background(), PrefsUpdate{Redetect: true}); err != nil {
		t.Fatal(err)
	}
	if st := m.Status(); st.BackendUnavailableReason != "" || st.Setup.Runtime != RuntimeContainer {
		t.Fatalf("after retry = %+v", st)
	}
	world, _ := m.Create(Spec{Name: "after"})
	if world.Backend != BackendDragonfly {
		t.Fatalf("new world backend = %q", world.Backend)
	}
}

func TestForcedBackendSurvivesRedetectAndSavedWorldsKeepTheirs(t *testing.T) {
	store := newTestStore(t)
	store.SetDefaultBackend(BackendDragonfly)
	old, _ := store.Create(Spec{Name: "old", Generator: GeneratorFlat})
	p := &Provisioner{Root: t.TempDir(), goos: "darwin", goarch: "arm64"}
	p.SetDetector(func(context.Context) RuntimeInfo { return RuntimeInfo{Kind: RuntimeContainer} })
	m := NewManager(store, Runners{}, nil)
	m.SetSetup(p)
	_, _ = m.Prefs(context.Background(), PrefsUpdate{Redetect: true})
	if w, _ := m.Create(Spec{Name: "n", Generator: GeneratorFlat}); w.Backend != BackendDragonfly {
		t.Fatalf("forced default changed to %q", w.Backend)
	}
	_, _ = m.Prefs(context.Background(), PrefsUpdate{Redetect: true})
	if got, _ := store.Get(old.ID); got.Backend != BackendDragonfly {
		t.Fatalf("saved world switched backend silently: %q", got.Backend)
	}
}

func TestPrefsPersistAndDefaultOff(t *testing.T) {
	store := newTestStore(t)
	if store.Prefs().DockerPromptDismissed {
		t.Fatal("prompt must default to shown")
	}
	yes := true
	if prefs, err := store.UpdatePrefs(PrefsUpdate{DockerPromptDismissed: &yes}); err != nil || !prefs.DockerPromptDismissed {
		t.Fatalf("update = %+v, %v", prefs, err)
	}
	reopened, _ := OpenStore(store.root)
	if !reopened.Prefs().DockerPromptDismissed {
		t.Fatal("dismissal not persisted")
	}
	if prefs, _ := reopened.UpdatePrefs(PrefsUpdate{Redetect: true}); !prefs.DockerPromptDismissed {
		t.Fatal("an update without the field must keep it")
	}
	if worlds, _ := reopened.List(); len(worlds) != 0 {
		t.Fatalf("prefs file listed as a world: %v", worlds)
	}
}

// An image without a digest drifts with upstream releases, so the runner refuses it before touching Docker.
func TestContainerRunnerRefusesUnpinnedImage(t *testing.T) {
	env, logPath := fakeDockerEnv(t)
	p := macProvisioner(t)
	_ = p.AcceptEULA()
	for _, image := range []string{"", "itzg/minecraft-bedrock-server:latest", "itzg/minecraft-bedrock-server:2026.9.2"} {
		runner := BDSRunner{Provisioner: p, Docker: os.Args[0], Env: env, Image: image}
		if _, err := runner.Start(context.Background(), testSpec()); !errors.Is(err, ErrImageNotPinned) {
			t.Fatalf("%q: %v", image, err)
		}
	}
	if raw, _ := os.ReadFile(logPath); len(raw) != 0 {
		t.Fatalf("docker ran for an unpinned image:\n%s", raw)
	}
}

// A daemon stopped since startup fails the open with a reason the client turns into the Retry prompt.
func TestContainerRunnerReportsStoppedDocker(t *testing.T) {
	env, _ := fakeDockerEnv(t, "LOCALWORLD_TEST_DOCKER_DOWN=1")
	p := macProvisioner(t)
	_ = p.AcceptEULA()
	runner := BDSRunner{Provisioner: p, Docker: os.Args[0], Env: env, Image: testImage}
	_, err := runner.Start(context.Background(), testSpec())
	if !errors.Is(err, ErrDockerNotRunning) || failureText(err) != ErrDockerNotRunning.Error() {
		t.Fatalf("err = %v", err)
	}
	if st := p.Status(); st.UnavailableReason != "docker_not_running" || st.State != SetupUnsupported {
		t.Fatalf("status = %+v", st)
	}
}

func TestPullProgressCountsLayers(t *testing.T) {
	pp := pullProgress{layers: map[string]bool{}}
	var done, total int
	for _, line := range []string{
		"2026.9.2: Pulling from itzg/minecraft-bedrock-server",
		"aaaaaaaaaaaa: Already exists",
		"bbbbbbbbbbbb: Pulling fs layer",
		"cccccccccccc: Pulling fs layer",
		"bbbbbbbbbbbb: Download complete",
		"bbbbbbbbbbbb: Pull complete",
		"bbbbbbbbbbbb: Pull complete",
		"Digest: sha256:0000",
	} {
		done, total = pp.line(line)
	}
	if done != 2 || total != 3 {
		t.Fatalf("progress = %d/%d", done, total)
	}
}

func writeExecutable(t *testing.T, path string, mode os.FileMode) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte("#!/bin/sh\n"), mode); err != nil {
		t.Fatal(err)
	}
}

// A Finder-launched app's PATH has no Docker folder, so the CLI must be found in its install folders.
func TestFindDockerSearchesInstallFoldersOutsidePATH(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("install folders are Unix paths")
	}
	t.Setenv("PATH", t.TempDir())
	home := t.TempDir()
	dirs := dockerDirs(home)
	for _, want := range []string{filepath.Join(home, ".orbstack", "bin"), filepath.Join(home, ".docker", "bin"), "/opt/homebrew/bin", "/Applications/Docker.app/Contents/Resources/bin"} {
		if !slices.Contains(dirs, want) {
			t.Fatalf("dockerDirs = %v, missing %s", dirs, want)
		}
	}
	notExec, orb := filepath.Join(home, "a"), filepath.Join(home, ".orbstack", "bin")
	writeExecutable(t, filepath.Join(notExec, "docker"), 0o600)
	writeExecutable(t, filepath.Join(orb, "docker"), 0o700)
	if got, ok := findDocker("docker", []string{filepath.Join(home, "missing"), notExec, orb}); !ok || got != filepath.Join(orb, "docker") {
		t.Fatalf("findDocker = %q, %v", got, ok)
	}
	if _, ok := findDocker("docker", []string{notExec}); ok {
		t.Fatal("a non-executable file must not count as docker")
	}
	if _, ok := findDocker(filepath.Join(home, "nope", "docker"), []string{orb}); ok {
		t.Fatal("an explicit path must not fall back to the install folders")
	}
}

// Docker shells out to credential helpers next to it, so its folder leads the child's PATH.
func TestDockerCommandRunsAbsolutePathWithItsFolderOnPATH(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("install folders are Unix paths")
	}
	t.Setenv("PATH", "/nonexistent")
	home := t.TempDir()
	t.Setenv("HOME", home)
	writeExecutable(t, filepath.Join(home, ".orbstack", "bin", "docker"), 0o700)
	cmd := dockerCommand(context.Background(), "docker", nil, "info")
	if !filepath.IsAbs(cmd.Path) {
		t.Fatalf("docker path = %q, want absolute", cmd.Path)
	}
	path := envValue(cmd.Env, "PATH")
	if first, _, _ := strings.Cut(path, string(os.PathListSeparator)); first != filepath.Dir(cmd.Path) || !strings.HasSuffix(path, "/nonexistent") {
		t.Fatalf("PATH = %q for %s", path, cmd.Path)
	}
}

// probeGate holds each runtime detection until the test answers it.
type probeGate struct{ calls chan chan RuntimeInfo }

func newProbeGate() *probeGate { return &probeGate{calls: make(chan chan RuntimeInfo, 4)} }

func (g *probeGate) detect(context.Context) RuntimeInfo {
	reply := make(chan RuntimeInfo)
	g.calls <- reply
	return <-reply
}

func (g *probeGate) next(t *testing.T) chan<- RuntimeInfo {
	t.Helper()
	select {
	case reply := <-g.calls:
		return reply
	case <-time.After(5 * time.Second):
		t.Fatal("no detection started")
		return nil
	}
}

var (
	dockerUp   = RuntimeInfo{Kind: RuntimeContainer, Reason: "docker up"}
	dockerDown = RuntimeInfo{Kind: RuntimeNone, Reason: "down", Unavailable: "docker_not_running"}
)

// pendingManager is a macOS manager whose startup detection is held by the returned gate.
func pendingManager(t *testing.T) (*Manager, *Provisioner, *probeGate) {
	t.Helper()
	gate := newProbeGate()
	store := newTestStore(t)
	store.SetDefaultBackend(BackendBDS) // core's optimistic default while the probe runs
	p := &Provisioner{Root: t.TempDir(), goos: "darwin", goarch: "arm64"}
	p.SetDetector(gate.detect)
	m := NewManager(store, Runners{}, nil)
	m.SetSetup(p)
	p.DetectInBackground(RuntimeInfo{Kind: RuntimeContainer, Reason: "checking"})
	return m, p, gate
}

func awaitSettled(t *testing.T, p *Provisioner) {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	if err := p.AwaitRuntime(ctx); err != nil {
		t.Fatal("detection never settled")
	}
}

// Core startup must not wait on `docker info`: prefs report checking_runtime until it lands, then the
// unavailable reason the launcher polls for.
func TestDetectInBackgroundReportsCheckingThenResult(t *testing.T) {
	m, p, gate := pendingManager(t)
	if _, err := m.Prefs(context.Background(), PrefsUpdate{}); err != nil {
		t.Fatal(err)
	}
	if st := m.Status(); st.Setup.State != SetupCheckingRuntime || st.Setup.Runtime != RuntimeContainer || st.BackendUnavailableReason != "" {
		t.Fatalf("while probing = %+v", st.Setup)
	}
	gate.next(t) <- dockerDown
	awaitSettled(t, p)
	if st := m.Status(); st.Setup.State != SetupUnsupported || st.BackendUnavailableReason != "docker_not_running" {
		t.Fatalf("after probe = %+v", st.Setup)
	}
}

// A selected BDS backend is refused after detection rather than converted to Dragonfly.
func TestCreateDuringPendingDetectionKeepsTheChosenBackend(t *testing.T) {
	m, p, gate := pendingManager(t)
	gate.next(t) <- dockerDown
	awaitSettled(t, p)
	if _, err := m.Create(Spec{Name: "chosen", Generator: GeneratorFlat}); !errors.Is(err, ErrBackendUnavailable) {
		t.Fatalf("selected BDS was not refused: %v", err)
	}
	if worlds, _ := m.List(); len(worlds) != 0 {
		t.Fatalf("saved a fallback world: %v", worlds)
	}
	if world, err := m.Create(Spec{Name: "normal", Backend: BackendDragonfly}); err != nil || world.Backend != BackendDragonfly {
		t.Fatalf("explicit Dragonfly without Docker: %+v, %v", world, err)
	}
}

// A probe that outlasts the wait refuses the create instead of saving a guess.
func TestCreateRefusesWhileDetectionOutlastsTheWait(t *testing.T) {
	m, p, gate := pendingManager(t)
	m.runtimeWait = 20 * time.Millisecond
	reply := gate.next(t)
	if _, err := m.Create(Spec{Name: "flat", Generator: GeneratorFlat}); !errors.Is(err, ErrRuntimePending) {
		t.Fatalf("err = %v", err)
	}
	if worlds, _ := m.List(); len(worlds) != 0 {
		t.Fatalf("saved %v while detection was pending", worlds)
	}
	reply <- dockerUp
	awaitSettled(t, p)
}

// A slow startup probe that lands after a newer Retry must not replace the Retry's result.
func TestOverlappingRedetectKeepsTheNewestResult(t *testing.T) {
	m, p, gate := pendingManager(t)
	startup := gate.next(t)
	retried := make(chan struct{})
	go func() {
		if _, err := m.Prefs(context.Background(), PrefsUpdate{Redetect: true}); err != nil {
			t.Error(err)
		}
		close(retried)
	}()
	gate.next(t) <- dockerUp
	<-retried
	startup <- dockerDown
	awaitSettled(t, p)
	if st := m.Status(); st.Setup.Runtime != RuntimeContainer || st.BackendUnavailableReason != "" {
		t.Fatalf("stale startup result won: %+v", st.Setup)
	}
	if world, err := m.Create(Spec{Name: "normal"}); err != nil || world.Backend != BackendBDS {
		t.Fatalf("world = %+v, %v", world, err)
	}
}

// A Flat world asks for Dragonfly outright, which no Docker probe can change, so it never waits on one.
func TestExplicitDragonflyCreateSkipsPendingDetection(t *testing.T) {
	m, p, gate := pendingManager(t)
	m.runtimeWait = time.Minute
	reply := gate.next(t)
	created := make(chan error, 1)
	go func() {
		_, err := m.Create(Spec{Name: "flat", Generator: GeneratorFlat, Backend: BackendDragonfly})
		created <- err
	}()
	select {
	case err := <-created:
		if err != nil {
			t.Fatal(err)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("explicit Dragonfly create waited on the Docker probe")
	}
	reply <- dockerUp
	awaitSettled(t, p)
}

// With Docker down and no Dragonfly binary, nothing can host the fallback, so Create refuses and saves nothing.
func TestCreateRefusesTheFallbackWhenItsServerIsMissing(t *testing.T) {
	m, p, gate := pendingManager(t)
	m.SetUnavailable(BackendDragonfly, errors.New("local world server binary not found"))
	gate.next(t) <- dockerDown
	awaitSettled(t, p)
	for _, spec := range []Spec{{Name: "auto", Generator: GeneratorFlat}, {Name: "explicit", Generator: GeneratorFlat, Backend: BackendDragonfly}} {
		if _, err := m.Create(spec); !errors.Is(err, ErrBackendUnavailable) {
			t.Fatalf("%s: err = %v", spec.Name, err)
		}
	}
	if worlds, _ := m.List(); len(worlds) != 0 {
		t.Fatalf("saved unopenable worlds %v", worlds)
	}
}
