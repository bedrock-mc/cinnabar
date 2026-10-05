package localworld

import (
	"archive/zip"
	"bytes"
	"context"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"net/url"
	"os"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"sync/atomic"
	"testing"
	"time"
)

func TestServerPropertiesForLocalPlay(t *testing.T) {
	world := World{ID: "0123456789abcdef", Name: "My\nWorld #1", GameMode: "creative", Generator: GeneratorFlat, Difficulty: "hard", Seed: -5}
	props := string(serverProperties(StartSpec{World: world, Options: OpenOptions{ViewDistance: 64}}, 5000, 1, false))
	for _, want := range []string{
		"server-name=MyWorld 1\n", "gamemode=creative\n", "difficulty=hard\n", "online-mode=false\n", "allow-list=false\n",
		"max-players=1\n", "server-port=5000\n", "server-udp-ports=127.0.0.1:5000:5000\n", "level-name=0123456789abcdef\n",
		"level-seed=-5\n", "level-type=FLAT\n", "view-distance=32\n", "tick-distance=12\n",
		"transport=" + string(TransportNetherNetHTTP) + "\n", "server-ip=" + localServerHost + "\n", "enable-lan-visibility=false\n",
	} {
		if !strings.Contains(props, want) {
			t.Fatalf("properties missing %q:\n%s", want, props)
		}
	}
	world.Generator = GeneratorNormal
	props = string(serverProperties(StartSpec{World: world}, 1, 1, false))
	if !strings.Contains(props, "level-type=DEFAULT\n") || !strings.Contains(props, "view-distance=10\n") {
		t.Fatalf("defaults wrong:\n%s", props)
	}
}

func TestBDSSupportMatrix(t *testing.T) {
	for goos, want := range map[string]bool{"windows": true, "linux": true, "darwin": false, "freebsd": false} {
		if got := bdsSupported(goos, "amd64"); got != want {
			t.Fatalf("%s/amd64 = %v", goos, got)
		}
	}
	if bdsSupported("linux", "arm64") {
		t.Fatal("arm64 has no BDS build")
	}
}

func buildZip(t *testing.T, files map[string]string) []byte {
	t.Helper()
	var buf bytes.Buffer
	w := zip.NewWriter(&buf)
	for name, body := range files {
		f, err := w.Create(name)
		if err != nil {
			t.Fatal(err)
		}
		_, _ = io.WriteString(f, body)
	}
	if err := w.Close(); err != nil {
		t.Fatal(err)
	}
	return buf.Bytes()
}

type fakeMojang struct {
	server *httptest.Server
	zip    []byte
	hits   atomic.Int32
	zipVer string
	ranges chan string // Range header of each archive request
	// stallAfter, when positive, sends that many archive bytes and then goes silent until the client gives up.
	stallAfter atomic.Int64
}

func newFakeMojang(t *testing.T, zipVer string, archive []byte) *fakeMojang {
	f := &fakeMojang{zip: archive, zipVer: zipVer, ranges: make(chan string, 16)}
	mux := http.NewServeMux()
	mux.HandleFunc("/links", func(w http.ResponseWriter, r *http.Request) {
		f.hits.Add(1)
		fmt.Fprintf(w, `{"result":{"links":[{"downloadType":"serverBedrockLinux","downloadUrl":"%s/bin-linux/bedrock-server-%s.zip"},{"downloadType":"serverBedrockWindows","downloadUrl":"%s/bin-win/bedrock-server-%s.zip"}]}}`,
			f.server.URL, zipVer, f.server.URL, zipVer)
	})
	mux.HandleFunc("/bin-linux/", func(w http.ResponseWriter, r *http.Request) {
		f.hits.Add(1)
		// minecraft.net resets Go's default agent, so downloads must name themselves.
		if r.UserAgent() != userAgent {
			http.Error(w, "agent", http.StatusForbidden)
			return
		}
		select {
		case f.ranges <- r.Header.Get("Range"):
		default:
		}
		if n := f.stallAfter.Load(); n > 0 {
			w.Header().Set("Content-Length", strconv.Itoa(len(f.zip)))
			_, _ = w.Write(f.zip[:n])
			w.(http.Flusher).Flush()
			<-r.Context().Done()
			return
		}
		http.ServeContent(w, r, "", time.Time{}, bytes.NewReader(f.zip))
	})
	f.server = httptest.NewServer(mux)
	t.Cleanup(f.server.Close)
	return f
}

func testProvisioner(t *testing.T, f *fakeMojang) *Provisioner {
	t.Helper()
	return &Provisioner{
		Root: filepath.Join(t.TempDir(), "bds"), goos: "linux", goarch: "amd64", VersionPrefix: "1.26.52",
		linksURL:  f.server.URL + "/links",
		allowHost: func(u *url.URL) bool { return u.Host == strings.TrimPrefix(f.server.URL, "http://") },
	}
}

func TestProvisionerRequiresEULAThenDownloadsVerifiesAndRecordsProvenance(t *testing.T) {
	f := newFakeMojang(t, "1.26.52.3", buildZip(t, map[string]string{"bedrock_server": "bin", "server.properties": "x"}))
	p := testProvisioner(t, f)
	if st := p.Status(); st.State != SetupEULARequired || st.EULAAccepted {
		t.Fatalf("status = %+v", st)
	}
	if _, err := p.Ensure(context.Background()); !errors.Is(err, ErrEULARequired) {
		t.Fatalf("ensure before EULA: %v", err)
	}
	if f.hits.Load() != 0 {
		t.Fatal("downloaded before the EULA was accepted")
	}
	if err := p.AcceptEULA(); err != nil {
		t.Fatal(err)
	}
	if st := p.Status(); st.State != SetupNotInstalled {
		t.Fatalf("status = %+v", st)
	}
	bin, err := p.Ensure(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if filepath.Base(filepath.Dir(bin)) != "1.26.52.3" {
		t.Fatalf("binary in %s", bin)
	}
	raw, err := os.ReadFile(filepath.Join(filepath.Dir(bin), "manifest.json"))
	if err != nil || !strings.Contains(string(raw), `"zip_sha256"`) || !strings.Contains(string(raw), "/bin-linux/bedrock-server-1.26.52.3.zip") {
		t.Fatalf("manifest = %s (%v)", raw, err)
	}
	if st := p.Status(); st.State != SetupReady || st.Version != "1.26.52.3" {
		t.Fatalf("status = %+v", st)
	}
	before := f.hits.Load()
	if _, err := p.Ensure(context.Background()); err != nil || f.hits.Load() != before {
		t.Fatalf("second ensure must reuse the install (hits %d -> %d, err %v)", before, f.hits.Load(), err)
	}
	if entries, _ := os.ReadDir(filepath.Join(p.Root, "downloads")); len(entries) != 0 {
		t.Fatalf("download left behind: %v", entries)
	}
}

func TestProvisionerRefusesVersionMismatch(t *testing.T) {
	f := newFakeMojang(t, "1.27.0.2", buildZip(t, map[string]string{"bedrock_server": "bin"}))
	p := testProvisioner(t, f)
	_ = p.AcceptEULA()
	if _, err := p.Ensure(context.Background()); err == nil || !strings.Contains(err.Error(), "1.26.52") {
		t.Fatalf("err = %v", err)
	}
	if st := p.Status(); st.State != SetupFailed || strings.Contains(st.Error, "http") {
		t.Fatalf("status = %+v", st)
	}
}

func TestProvisionerRejectsZipSlipAndMissingBinary(t *testing.T) {
	for name, files := range map[string]map[string]string{
		"slip":   {"../evil": "x", "bedrock_server": "bin"},
		"nobin":  {"readme.txt": "x"},
		"absent": {},
	} {
		f := newFakeMojang(t, "1.26.52.3", buildZip(t, files))
		p := testProvisioner(t, f)
		_ = p.AcceptEULA()
		if _, err := p.Ensure(context.Background()); err == nil {
			t.Fatalf("%s: expected failure", name)
		}
		if _, _, ok := p.installed(); ok {
			t.Fatalf("%s: partial install must not count", name)
		}
		if _, err := os.Stat(filepath.Join(filepath.Dir(p.Root), "evil")); err == nil {
			t.Fatalf("%s: archive escaped the install directory", name)
		}
	}
}

func TestProvisionerRefusesUnofficialHostsAndUnsupportedPlatforms(t *testing.T) {
	p := &Provisioner{Root: t.TempDir(), goos: "linux", goarch: "amd64", VersionPrefix: "1.26.52", linksURL: "https://evil.example/links"}
	_ = p.AcceptEULA()
	if _, err := p.Ensure(context.Background()); err == nil || !strings.Contains(err.Error(), "non-official") {
		t.Fatalf("err = %v", err)
	}
	mac := &Provisioner{Root: t.TempDir(), goos: "darwin", goarch: "arm64"}
	if _, err := mac.Ensure(context.Background()); !errors.Is(err, ErrBackendUnavailable) {
		t.Fatalf("darwin: %v", err)
	}
	if mac.Status().State != SetupUnsupported {
		t.Fatalf("status = %+v", mac.Status())
	}
}

func TestExactVersionOverrideMustMatchClientVersion(t *testing.T) {
	p := &Provisioner{Root: t.TempDir(), goos: "linux", goarch: "amd64", Version: "1.27.1.0", VersionPrefix: "1.26.52"}
	_ = p.AcceptEULA()
	if _, err := p.Ensure(context.Background()); err == nil || !strings.Contains(err.Error(), "does not match") {
		t.Fatalf("err = %v", err)
	}
}

// Without a pinned version nothing is fetched: the current download may speak another protocol.
func TestProvisionerNeedsAPinnedVersion(t *testing.T) {
	f := newFakeMojang(t, "1.26.52.3", buildZip(t, map[string]string{"bedrock_server": "bin"}))
	p := testProvisioner(t, f)
	p.VersionPrefix = ""
	_ = p.AcceptEULA()
	if _, err := p.Ensure(context.Background()); err == nil || !strings.Contains(err.Error(), "pinned") {
		t.Fatalf("err = %v", err)
	}
	if f.hits.Load() != 0 {
		t.Fatal("contacted the download service without a pinned version")
	}
	if got := (&Provisioner{Version: "1.26.52.3"}).prefix(); got != "1.26.52" {
		t.Fatalf("prefix from version = %q", got)
	}
}

func installFakeBDS(t *testing.T) *Provisioner {
	t.Helper()
	p := &Provisioner{Root: filepath.Join(t.TempDir(), "bds"), goos: "linux", goarch: "amd64", Version: "1.26.52.3"}
	if err := p.AcceptEULA(); err != nil {
		t.Fatal(err)
	}
	dir := filepath.Join(p.Root, "1.26.52.3")
	if err := os.MkdirAll(dir, 0o700); err != nil {
		t.Fatal(err)
	}
	_ = os.WriteFile(filepath.Join(dir, "manifest.json"), []byte("{}"), 0o600)
	src, err := os.Open(os.Args[0])
	if err != nil {
		t.Fatal(err)
	}
	defer src.Close()
	dst, err := os.OpenFile(filepath.Join(dir, "bedrock_server"), os.O_CREATE|os.O_WRONLY, 0o755)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := io.Copy(dst, src); err != nil {
		t.Fatal(err)
	}
	_ = dst.Close()
	return p
}

func TestBDSRunnerLifecycleWithFakeServer(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("uses a symlink to the copied test binary")
	}
	p := installFakeBDS(t)
	runner := BDSRunner{Provisioner: p, Env: []string{helperEnv + "=bds"}, StartTimeout: 20e9}
	spec := testSpec()
	spec.Dir = t.TempDir()
	inst, err := runner.Start(context.Background(), spec)
	if err != nil {
		t.Fatal(err)
	}
	installDir := filepath.Join(p.Root, "1.26.52.3")
	if raw, err := os.ReadFile(filepath.Join(installDir, "server.properties")); err != nil || !strings.Contains(string(raw), "online-mode=false") {
		t.Fatalf("server.properties = %s (%v)", raw, err)
	}
	link := filepath.Join(installDir, "worlds", spec.World.ID)
	if resolved, err := filepath.EvalSymlinks(link); err != nil || resolved != mustEval(t, filepath.Join(spec.Dir, "db")) {
		t.Fatalf("world link resolves to %q (%v)", resolved, err)
	}
	if c, ok := inst.(interface{ CanPause() bool }); !ok || c.CanPause() {
		t.Fatal("BDS must report that it cannot pause")
	}
	if err := inst.SetPaused(true); err != nil {
		t.Fatalf("pause must be a harmless no-op: %v", err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 10e9)
	defer cancel()
	if err := inst.Stop(ctx); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Lstat(link); err == nil {
		t.Fatal("world link not removed after stop")
	}
	if _, err := os.Stat(filepath.Join(spec.Dir, "db")); err != nil {
		t.Fatalf("world data must survive stop: %v", err)
	}
}

func mustEval(t *testing.T, path string) string {
	t.Helper()
	resolved, err := filepath.EvalSymlinks(path)
	if err != nil {
		t.Fatal(err)
	}
	return resolved
}

func TestBDSRunnerStartupFailuresCleanUpLink(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("symlink based")
	}
	p := installFakeBDS(t)
	runner := BDSRunner{Provisioner: p, Env: []string{helperEnv + "=crash"}, StartTimeout: 20e9}
	spec := testSpec()
	spec.Dir = t.TempDir()
	if _, err := runner.Start(context.Background(), spec); err == nil {
		t.Fatal("expected startup failure")
	}
	if _, err := os.Lstat(filepath.Join(p.Root, "1.26.52.3", "worlds", spec.World.ID)); err == nil {
		t.Fatal("link left behind after failed start")
	}
}

func TestBDSRunnerNeedsEULAAndProvisioner(t *testing.T) {
	p := &Provisioner{Root: t.TempDir(), goos: "linux", goarch: "amd64"}
	if _, err := (BDSRunner{Provisioner: p}).Start(context.Background(), testSpec()); !errors.Is(err, ErrEULARequired) {
		t.Fatalf("err = %v", err)
	}
	if _, err := (BDSRunner{}).Start(context.Background(), testSpec()); !errors.Is(err, ErrBackendUnavailable) {
		t.Fatalf("err = %v", err)
	}
}

func TestManagerGatesBDSWorldsOnEULAAndPlatform(t *testing.T) {
	store := newTestStore(t)
	store.SetDefaultBackend(BackendBDS)
	runner := &fakeRunner{}
	m := NewManager(store, Runners{BackendBDS: runner, BackendDragonfly: runner}, nil)
	t.Cleanup(m.Shutdown)
	world, err := store.Create(Spec{Name: "b"})
	if err != nil || world.Backend != BackendBDS {
		t.Fatalf("world = %+v, %v", world, err)
	}
	if err := m.Open(world.ID); !errors.Is(err, ErrBackendUnavailable) {
		t.Fatalf("no setup configured: %v", err)
	}
	p := &Provisioner{Root: filepath.Join(t.TempDir(), "bds"), goos: "linux", goarch: "amd64"}
	m.SetSetup(p)
	if err := m.Open(world.ID); !errors.Is(err, ErrEULARequired) {
		t.Fatalf("before EULA: %v", err)
	}
	if st := m.Status(); st.Setup == nil || st.Setup.State != SetupEULARequired || st.State != StateIdle {
		t.Fatalf("status = %+v", st)
	}
	if err := m.AcceptEULA(); err != nil {
		t.Fatal(err)
	}
	if err := m.Open(world.ID, OpenOptions{ViewDistance: 8}); err != nil {
		t.Fatal(err)
	}
	if st := waitState(t, m, StateRunning); st.Backend != BackendBDS || !st.PauseSupported {
		t.Fatalf("status = %+v", st)
	}

	m.SetSetup(&Provisioner{Root: t.TempDir(), goos: "darwin", goarch: "arm64"})
	if _, err := m.Create(Spec{Name: "x", Backend: BackendBDS, Generator: GeneratorFlat}); !errors.Is(err, ErrBackendUnavailable) {
		t.Fatalf("explicit BDS on unsupported platform: %v", err)
	}
	if _, err := m.Create(Spec{Name: "x", Backend: BackendDragonfly, Generator: GeneratorFlat}); err != nil {
		t.Fatalf("dragonfly stays available: %v", err)
	}
}

// Normal worlds are vanilla terrain, which only BDS generates; dragonfly never approximates it.
func TestNormalWorldsRouteToBDSOrAreUnavailable(t *testing.T) {
	store := newTestStore(t)
	store.SetDefaultBackend(BackendDragonfly)
	runner := &fakeRunner{}
	m := NewManager(store, Runners{BackendBDS: runner, BackendDragonfly: runner}, nil)
	t.Cleanup(m.Shutdown)
	m.SetSetup(&Provisioner{Root: t.TempDir(), goos: "linux", goarch: "amd64"})

	if w, err := m.Create(Spec{Name: "n"}); err != nil || w.Backend != BackendBDS || w.Generator != GeneratorNormal {
		t.Fatalf("default world = %+v, %v", w, err)
	}
	if w, err := m.Create(Spec{Name: "f", Generator: GeneratorFlat}); err != nil || w.Backend != BackendDragonfly {
		t.Fatalf("flat world = %+v, %v", w, err)
	}
	if _, err := m.Create(Spec{Name: "d", Backend: BackendDragonfly}); !errors.Is(err, ErrVanillaNeedsBDS) {
		t.Fatalf("normal on dragonfly: %v", err)
	}
	m.SetSetup(&Provisioner{Root: t.TempDir(), goos: "darwin", goarch: "arm64"})
	if _, err := m.Create(Spec{Name: "u"}); !errors.Is(err, ErrVanillaNeedsBDS) {
		t.Fatalf("normal where BDS cannot run: %v", err)
	}

	legacy := World{ID: "0123456789abcdef", Name: "old", GameMode: GameModeSurvival, Generator: GeneratorNormal, Difficulty: DifficultyNormal, Backend: BackendDragonfly}
	if err := store.write(legacy); err != nil {
		t.Fatal(err)
	}
	if err := m.Open(legacy.ID); !errors.Is(err, ErrVanillaNeedsBDS) {
		t.Fatalf("legacy normal dragonfly world opened: %v", err)
	}
	if _, err := (ProcessRunner{Binary: "unused"}).Start(context.Background(), StartSpec{World: legacy}); !errors.Is(err, ErrVanillaNeedsBDS) {
		t.Fatalf("process runner accepted a normal world: %v", err)
	}
}

type noPauseInstance struct{ *fakeInstance }

func (noPauseInstance) CanPause() bool { return false }

type noPauseRunner struct{}

func (noPauseRunner) Start(context.Context, StartSpec) (Instance, error) {
	return noPauseInstance{newFakeInstance()}, nil
}

func TestManagerReportsPauseUnsupportedAndNeverMarksPaused(t *testing.T) {
	m, world := newTestManager(t, noPauseRunner{})
	_ = m.Open(world.ID)
	st := waitState(t, m, StateRunning)
	if st.PauseSupported {
		t.Fatal("pause must be reported unsupported")
	}
	if err := m.SetPaused(true); err != nil || m.Status().Paused {
		t.Fatalf("pause = %v, status %+v", err, m.Status())
	}
}

func TestRunnersRejectUnknownBackend(t *testing.T) {
	if _, err := (Runners{}).Start(context.Background(), StartSpec{World: World{Backend: "x"}}); !errors.Is(err, ErrBackendUnavailable) {
		t.Fatalf("err = %v", err)
	}
}

func TestLegacyWorldsWithoutBackendLoadAsDragonfly(t *testing.T) {
	store := newTestStore(t)
	world, _ := store.Create(Spec{Name: "old", Generator: GeneratorFlat})
	dir, _ := store.Dir(world.ID)
	raw, _ := os.ReadFile(filepath.Join(dir, metaFile))
	raw = bytes.ReplaceAll(raw, []byte(`"backend": "dragonfly",`), nil)
	if err := os.WriteFile(filepath.Join(dir, metaFile), raw, 0o600); err != nil {
		t.Fatal(err)
	}
	got, err := store.Get(world.ID)
	if err != nil || got.Backend != BackendDragonfly {
		t.Fatalf("got %+v, %v", got, err)
	}
}
