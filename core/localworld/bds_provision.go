package localworld

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"regexp"
	"runtime"
	"strings"
	"sync"
	"time"
)

const (
	linksAPI        = "https://net-secondary.web.minecraft-services.net/api/v1.0/download/links"
	directURLFormat = "https://www.minecraft.net/bedrockdedicatedserver/bin-%s/bedrock-server-%s.zip"
	// userAgent names the downloader; minecraft.net resets requests carrying Go's default agent.
	userAgent = "Cinnabar-local-worlds"
)

var zipVersion = regexp.MustCompile(`bedrock-server-(\d+(?:\.\d+)+)\.zip$`)

// SetupState is the lifecycle of the dedicated-server installation.
type SetupState string

const (
	SetupUnsupported  SetupState = "unsupported"
	SetupEULARequired SetupState = "eula_required"
	SetupNotInstalled SetupState = "not_installed"
	// SetupCheckingRuntime and SetupPullingImage are the container runtime's steps before the download.
	SetupCheckingRuntime SetupState = "checking_runtime"
	SetupPullingImage    SetupState = "pulling_image"
	SetupDownloading     SetupState = "downloading"
	SetupUnpacking       SetupState = "unpacking"
	SetupReady           SetupState = "ready"
	SetupFailed          SetupState = "failed"
)

// SetupStatus reports BDS acquisition; Error never carries paths.
type SetupStatus struct {
	State      SetupState `json:"state"`
	Version    string     `json:"version,omitempty"`
	BytesDone  int64      `json:"bytes_done"`
	BytesTotal int64      `json:"bytes_total"`
	// LayersDone and LayersTotal count image layers while pulling; Docker reports no bytes without a TTY.
	LayersDone   int    `json:"layers_done,omitempty"`
	LayersTotal  int    `json:"layers_total,omitempty"`
	EULAAccepted bool   `json:"eula_accepted"`
	Error        string `json:"error,omitempty"`
	// Runtime is how BDS runs on this machine (native, container, none) and Reason says why.
	Runtime string `json:"runtime"`
	Reason  string `json:"reason,omitempty"`
	// UnavailableReason is docker_missing or docker_not_running when BDS cannot run only for want of Docker.
	UnavailableReason string `json:"backend_unavailable_reason,omitempty"`
}

// Setup is the dedicated-server installer as seen by the Manager.
type Setup interface {
	Status() SetupStatus
	AcceptEULA() error
	Redetect(ctx context.Context) RuntimeInfo
	// AwaitRuntime returns once no runtime detection is in flight, or with ctx's error.
	AwaitRuntime(ctx context.Context) error
	// OnDetected registers fn to run for each detection result that becomes current.
	OnDetected(fn func(RuntimeInfo))
}

// manifest records where an installed build came from.
type manifest struct {
	Version       string `json:"version"`
	URL           string `json:"url"`
	ZipSHA256     string `json:"zip_sha256"`
	ZipBytes      int64  `json:"zip_bytes"`
	Platform      string `json:"platform"`
	DownloadedAt  int64  `json:"downloaded_unix"`
	ClientVersion string `json:"client_version_prefix"`
}

// Provisioner downloads the official Bedrock Dedicated Server on first use, after explicit EULA acceptance.
// Nothing is bundled or committed; builds live in Root/<version>/ with a provenance manifest.
type Provisioner struct {
	Root string
	// Version is the exact build (the client passes the target manifest's server_version), fetched from its
	// versioned official URL; without it the download API's current build must match VersionPrefix.
	Version       string
	VersionPrefix string // default: Version's first three components
	Client        *http.Client
	Log           *slog.Logger

	// test hooks
	stallTimeout time.Duration
	linksURL     string
	goos         string
	goarch       string
	allowHost    func(*url.URL) bool

	runtime     string // set by SetRuntime; empty derives from the platform
	reason      string
	unavailable string
	detect      func(context.Context) RuntimeInfo // used by Redetect
	onDetected  func(RuntimeInfo)
	probing     int           // detections in flight
	settled     chan struct{} // closed when the last in-flight detection lands; nil while none runs
	started     uint64        // generation of the newest detection or SetRuntime
	applied     uint64        // generation of the runtime in effect; older results are dropped
	detectMu    sync.Mutex    // keeps results and their onDetected calls in generation order

	ensureMu sync.Mutex
	mu       sync.Mutex
	op       SetupState // the step in progress while a BDS world starts
	done     int64
	total    int64
	layers   [2]int // pulled, total
	version  string
	lastErr  string
}

func (p *Provisioner) platform() (goos, arch string) {
	goos, arch = p.goos, p.goarch
	if goos == "" {
		goos = runtime.GOOS
	}
	if arch == "" {
		arch = runtime.GOARCH
	}
	return
}

// PlatformSupportsBDS reports whether Mojang ships a dedicated server for this OS and architecture.
func PlatformSupportsBDS() bool {
	return bdsSupported(runtime.GOOS, runtime.GOARCH)
}

func bdsSupported(goos, arch string) bool {
	return (goos == "windows" || goos == "linux") && arch == "amd64"
}

// Runtimes for the dedicated server.
const (
	RuntimeNative    = "native"    // official zip run directly (Windows, Linux x86-64)
	RuntimeContainer = "container" // Linux build in a Docker-compatible container (macOS)
	RuntimeNone      = "none"
)

// SetRuntime records how BDS will run and why, as detected by DetectRuntime.
// It supersedes detections still in flight.
func (p *Provisioner) SetRuntime(info RuntimeInfo) {
	p.mu.Lock()
	p.started++
	p.applied = p.started
	p.runtime, p.reason, p.unavailable = info.Kind, info.Reason, info.Unavailable
	p.mu.Unlock()
}

// SetDetector sets the probe Redetect runs.
func (p *Provisioner) SetDetector(detect func(context.Context) RuntimeInfo) {
	p.mu.Lock()
	p.detect = detect
	p.mu.Unlock()
}

// OnDetected registers fn to run, in order, for each detection result that becomes current.
func (p *Provisioner) OnDetected(fn func(RuntimeInfo)) {
	p.mu.Lock()
	p.onDetected = fn
	p.mu.Unlock()
}

// Redetect re-probes the runtime (for example after the user starts Docker) and returns the runtime in effect,
// which is a newer probe's result when one overtook this one.
func (p *Provisioner) Redetect(ctx context.Context) RuntimeInfo {
	gen, detect := p.beginDetect()
	if detect == nil {
		p.finishDetect(gen, nil)
	} else {
		info := detect(ctx)
		p.finishDetect(gen, &info)
	}
	kind, reason, unavailable := p.runtimeInfo()
	return RuntimeInfo{kind, reason, unavailable}
}

// DetectInBackground assumes the given runtime until the detector's result lands; Status reports
// checking_runtime meanwhile.
func (p *Provisioner) DetectInBackground(assume RuntimeInfo) {
	p.SetRuntime(assume)
	gen, detect := p.beginDetect()
	go func() {
		if detect == nil {
			p.finishDetect(gen, nil)
			return
		}
		info := detect(context.Background())
		p.finishDetect(gen, &info)
	}()
}

// AwaitRuntime returns once no detection is in flight, or with ctx's error.
func (p *Provisioner) AwaitRuntime(ctx context.Context) error {
	p.mu.Lock()
	settled := p.settled
	p.mu.Unlock()
	if settled == nil {
		return nil
	}
	select {
	case <-settled:
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
}

func (p *Provisioner) beginDetect() (uint64, func(context.Context) RuntimeInfo) {
	p.mu.Lock()
	defer p.mu.Unlock()
	p.started++
	if p.probing == 0 {
		p.settled = make(chan struct{})
	}
	p.probing++
	return p.started, p.detect
}

// finishDetect applies info (nil for no result) unless a newer generation already took effect.
func (p *Provisioner) finishDetect(gen uint64, info *RuntimeInfo) {
	p.detectMu.Lock()
	p.mu.Lock()
	fresh := info != nil && gen > p.applied
	if fresh {
		p.applied = gen
		p.runtime, p.reason, p.unavailable = info.Kind, info.Reason, info.Unavailable
	}
	hook := p.onDetected
	p.mu.Unlock()
	if fresh && hook != nil {
		hook(*info)
	}
	p.detectMu.Unlock()
	p.mu.Lock()
	if p.probing--; p.probing == 0 {
		close(p.settled)
		p.settled = nil
	}
	p.mu.Unlock()
}

func (p *Provisioner) runtimeInfo() (kind, reason, unavailable string) {
	p.mu.Lock()
	kind, reason, unavailable = p.runtime, p.reason, p.unavailable
	p.mu.Unlock()
	if kind == "" {
		goos, arch := p.platform()
		if bdsSupported(goos, arch) {
			return RuntimeNative, "native Bedrock Dedicated Server", ""
		}
		return RuntimeNone, "Bedrock Dedicated Server has no build for this platform", ""
	}
	return kind, reason, unavailable
}

func (p *Provisioner) runtimeKind() (kind, reason string) {
	kind, reason, _ = p.runtimeInfo()
	return kind, reason
}

// prefix is the release line the client can join; empty when nothing is pinned.
func (p *Provisioner) prefix() string {
	if p.VersionPrefix != "" {
		return p.VersionPrefix
	}
	if parts := strings.Split(p.Version, "."); len(parts) >= 3 {
		return strings.Join(parts[:3], ".")
	}
	return ""
}

func (p *Provisioner) log() *slog.Logger {
	if p.Log != nil {
		return p.Log
	}
	return slog.Default()
}

func (p *Provisioner) binaryName() string {
	if goos, _ := p.platform(); goos == "windows" {
		return "bedrock_server.exe"
	}
	return "bedrock_server"
}

func (p *Provisioner) eulaPath() string { return filepath.Join(p.Root, "eula.json") }

func (p *Provisioner) eulaAccepted() bool {
	_, err := os.Stat(p.eulaPath())
	return err == nil
}

// AcceptEULA records the user's acceptance of the Minecraft EULA and privacy policy.
func (p *Provisioner) AcceptEULA() error {
	if err := os.MkdirAll(p.Root, 0o700); err != nil {
		return fmt.Errorf("localworld: create server directory: %w", err)
	}
	raw, _ := json.Marshal(map[string]any{"accepted_unix": time.Now().Unix(), "terms": "https://minecraft.net/eula"})
	return os.WriteFile(p.eulaPath(), raw, 0o600)
}

// installed returns the binary of an installed build whose version matches the target.
func (p *Provisioner) installed() (binary, version string, ok bool) {
	entries, err := os.ReadDir(p.Root)
	if err != nil {
		return "", "", false
	}
	for _, entry := range entries {
		if !entry.IsDir() || !p.versionMatches(entry.Name()) {
			continue
		}
		if p.Version != "" && entry.Name() != p.Version {
			continue
		}
		dir := filepath.Join(p.Root, entry.Name())
		if _, err := os.Stat(filepath.Join(dir, "manifest.json")); err != nil {
			continue
		}
		bin := filepath.Join(dir, p.binaryName())
		if info, err := os.Stat(bin); err == nil && !info.IsDir() {
			return bin, entry.Name(), true
		}
	}
	return "", "", false
}

func (p *Provisioner) versionMatches(version string) bool {
	prefix := p.prefix()
	return prefix != "" && (version == prefix || strings.HasPrefix(version, prefix+"."))
}

// Status reports the installation state for the client.
func (p *Provisioner) Status() SetupStatus {
	kind, reason, unavailable := p.runtimeInfo()
	accepted := p.eulaAccepted()
	p.mu.Lock()
	op, done, total, layers, version, lastErr, probing := p.op, p.done, p.total, p.layers, p.version, p.lastErr, p.probing > 0
	p.mu.Unlock()
	status := SetupStatus{Version: version, BytesDone: done, BytesTotal: total, LayersDone: layers[0], LayersTotal: layers[1],
		EULAAccepted: accepted, Runtime: kind, Reason: reason, UnavailableReason: unavailable}
	switch {
	case kind == RuntimeNone:
		status.State = SetupUnsupported
	case op != "":
		status.State = op
	case probing:
		status.State = SetupCheckingRuntime
	default:
		if _, v, ok := p.installed(); ok {
			status.State, status.Version = SetupReady, v
		} else if !accepted {
			status.State = SetupEULARequired
		} else if lastErr != "" {
			status.State, status.Error = SetupFailed, lastErr
		} else {
			status.State = SetupNotInstalled
		}
	}
	return status
}

func (p *Provisioner) setOp(op SetupState, version string, done, total int64) {
	p.mu.Lock()
	p.op, p.version, p.done, p.total, p.layers = op, version, done, total, [2]int{}
	p.mu.Unlock()
}

func (p *Provisioner) setLayers(done, total int) {
	p.mu.Lock()
	p.layers = [2]int{done, total}
	p.mu.Unlock()
}

func (p *Provisioner) fail(err error) error {
	return p.failWith("dedicated server download failed", err)
}

// failWith ends the current step; message is the path-free text the client shows.
func (p *Provisioner) failWith(message string, err error) error {
	p.log().Error("dedicated server setup failed", "error", err)
	p.mu.Lock()
	p.op, p.lastErr = "", message
	p.mu.Unlock()
	return err
}

// Ensure returns the path of the server binary, downloading and unpacking it first if needed.
// The container runtime gets the Linux build, which runs inside the container.
func (p *Provisioner) Ensure(ctx context.Context) (string, error) {
	p.ensureMu.Lock()
	defer p.ensureMu.Unlock()
	if kind, _ := p.runtimeKind(); kind != RuntimeNative && kind != RuntimeContainer {
		return "", ErrBackendUnavailable
	}
	if !p.eulaAccepted() {
		return "", ErrEULARequired
	}
	if bin, _, ok := p.installed(); ok {
		return bin, nil
	}
	p.mu.Lock()
	p.lastErr = ""
	p.mu.Unlock()
	version, link, err := p.resolve(ctx)
	if err != nil {
		return "", p.fail(err)
	}
	zipPath, sum, size, err := p.download(ctx, version, link)
	if err != nil {
		return "", p.fail(err)
	}
	defer os.Remove(zipPath) // kept only on a failed download, to resume
	p.setOp(SetupUnpacking, version, size, size)
	bin, err := p.unpack(zipPath, version, link, sum, size)
	if err != nil {
		return "", p.fail(err)
	}
	p.pruneOldInstalls(version)
	p.setOp("", version, size, size)
	return bin, nil
}

func (p *Provisioner) hostAllowed(u *url.URL) bool {
	if p.allowHost != nil {
		return p.allowHost(u)
	}
	host := strings.ToLower(u.Hostname())
	return u.Scheme == "https" && (host == "minecraft.net" || strings.HasSuffix(host, ".minecraft.net") ||
		strings.HasSuffix(host, ".minecraft-services.net"))
}

func (p *Provisioner) client() *http.Client {
	if p.Client != nil {
		return p.Client
	}
	return &http.Client{CheckRedirect: func(req *http.Request, _ []*http.Request) error {
		if !p.hostAllowed(req.URL) {
			return fmt.Errorf("localworld: refusing redirect to %s", req.URL.Host)
		}
		return nil
	}}
}

var _ Setup = (*Provisioner)(nil)

// get fetches rawURL from byte from onwards; a ranged request also accepts 206 and 416.
func (p *Provisioner) get(ctx context.Context, rawURL string, from int64) (*http.Response, error) {
	u, err := url.Parse(rawURL)
	if err != nil || !p.hostAllowed(u) {
		return nil, fmt.Errorf("localworld: refusing non-official download URL %q", rawURL)
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, rawURL, nil)
	if err != nil {
		return nil, err
	}
	req.Header.Set("User-Agent", userAgent)
	if from > 0 {
		req.Header.Set("Range", fmt.Sprintf("bytes=%d-", from))
	}
	resp, err := p.client().Do(req)
	if err != nil {
		return nil, err
	}
	ranged := from > 0 && (resp.StatusCode == http.StatusPartialContent || resp.StatusCode == http.StatusRequestedRangeNotSatisfiable)
	if resp.StatusCode != http.StatusOK && !ranged {
		resp.Body.Close()
		return nil, fmt.Errorf("localworld: download %s: %s", u.Host, resp.Status)
	}
	return resp, nil
}

// resolve returns the target build's version and official zip URL.
func (p *Provisioner) resolve(ctx context.Context) (version, link string, err error) {
	goos, _ := p.platform()
	kind, dir := "serverBedrockLinux", "linux"
	if goos == "windows" {
		kind, dir = "serverBedrockWindows", "win"
	}
	if p.Version != "" {
		if !p.versionMatches(p.Version) {
			return "", "", fmt.Errorf("localworld: server version %s does not match client version %s", p.Version, p.prefix())
		}
		return p.Version, fmt.Sprintf(directURLFormat, dir, p.Version), nil
	}
	if p.prefix() == "" {
		return "", "", errors.New("localworld: no dedicated server version is pinned")
	}
	api := p.linksURL
	if api == "" {
		api = linksAPI
	}
	resp, err := p.get(ctx, api, 0)
	if err != nil {
		return "", "", err
	}
	defer resp.Body.Close()
	var body struct {
		Result struct {
			Links []struct {
				DownloadType string `json:"downloadType"`
				DownloadURL  string `json:"downloadUrl"`
			} `json:"links"`
		} `json:"result"`
	}
	if err := json.NewDecoder(io.LimitReader(resp.Body, 1<<20)).Decode(&body); err != nil {
		return "", "", fmt.Errorf("localworld: decode download links: %w", err)
	}
	for _, item := range body.Result.Links {
		if item.DownloadType != kind {
			continue
		}
		match := zipVersion.FindStringSubmatch(item.DownloadURL)
		if match == nil {
			continue
		}
		if !p.versionMatches(match[1]) {
			return "", "", fmt.Errorf("localworld: current dedicated server is %s but the client needs %s; set an exact server version", match[1], p.prefix())
		}
		return match[1], item.DownloadURL, nil
	}
	return "", "", errors.New("localworld: no dedicated server download listed for this platform")
}

// RuntimeInfo is the detected way to run BDS.
type RuntimeInfo struct {
	Kind, Reason string
	// Unavailable is docker_missing or docker_not_running when Kind is none for want of Docker.
	Unavailable string
}

// DetectRuntime prefers native BDS, then a Docker-compatible container (probed with `docker info`), else none.
func DetectRuntime(ctx context.Context, docker string) RuntimeInfo {
	return detectRuntime(ctx, runtime.GOOS, runtime.GOARCH, docker, nil)
}

func detectRuntime(ctx context.Context, goos, arch, docker string, env []string) RuntimeInfo {
	if bdsSupported(goos, arch) {
		return RuntimeInfo{Kind: RuntimeNative, Reason: "native Bedrock Dedicated Server"}
	}
	if docker == "" {
		docker = "docker"
	}
	if _, found := lookupDocker(docker); !found {
		return RuntimeInfo{RuntimeNone, "no native Bedrock Dedicated Server for this platform and Docker is not installed", "docker_missing"}
	}
	probe, cancel := context.WithTimeout(ctx, 20*time.Second)
	defer cancel()
	if err := dockerCommand(probe, docker, env, "info").Run(); err == nil {
		return RuntimeInfo{Kind: RuntimeContainer, Reason: "no native Bedrock Dedicated Server for this platform; running the Linux build in a container"}
	}
	return RuntimeInfo{RuntimeNone, "no native Bedrock Dedicated Server for this platform and Docker is not running", "docker_not_running"}
}
