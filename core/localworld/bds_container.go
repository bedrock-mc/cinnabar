package localworld

import (
	"bufio"
	"context"
	"errors"
	"fmt"
	"log/slog"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"runtime"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/df-mc/go-nethernet/discovery"
)

const (
	containerStartTimeout = 10 * time.Minute // first start of a world generates spawn chunks under emulation
	containerStopSeconds  = 25               // inside the manager's 30s stop budget
)

// ErrImageNotPinned refuses an image without a digest: an unpinned image drifts from the joinable protocol.
var ErrImageNotPinned = errors.New("the server container image is not pinned to a digest")

func (r BDSRunner) dockerBin() string {
	if r.Docker != "" {
		return r.Docker
	}
	return "docker"
}

// pinnedImage is Image when it names an exact digest.
func (r BDSRunner) pinnedImage() (string, error) {
	if !strings.Contains(r.Image, "@sha256:") {
		return "", ErrImageNotPinned
	}
	return r.Image, nil
}

func (r BDSRunner) dockerCmd(ctx context.Context, args ...string) *exec.Cmd {
	return dockerCommand(ctx, r.dockerBin(), r.Env, args...)
}

// dockerDirs are where Docker Desktop, Homebrew and OrbStack put the CLI; a Finder-launched app's PATH has none.
func dockerDirs(home string) []string {
	dirs := []string{"/usr/local/bin", "/opt/homebrew/bin"}
	if home != "" {
		dirs = append(dirs, filepath.Join(home, ".docker", "bin"))
	}
	dirs = append(dirs, "/Applications/Docker.app/Contents/Resources/bin")
	if home != "" {
		dirs = append(dirs, filepath.Join(home, ".orbstack", "bin"))
	}
	return dirs
}

// findDocker returns the CLI's absolute path from PATH, else from dirs; a name with a separator is used as given.
func findDocker(name string, dirs []string) (string, bool) {
	if path, err := exec.LookPath(name); err == nil {
		if abs, err := filepath.Abs(path); err == nil {
			return abs, true
		}
		return path, true
	}
	if strings.ContainsAny(name, `/\`) {
		return "", false
	}
	for _, dir := range dirs {
		candidate := filepath.Join(dir, name)
		if info, err := os.Stat(candidate); err == nil && info.Mode().IsRegular() && info.Mode().Perm()&0o111 != 0 {
			return candidate, true
		}
	}
	return "", false
}

func lookupDocker(name string) (string, bool) {
	home, _ := os.UserHomeDir()
	return findDocker(name, dockerDirs(home))
}

// dockerCommand runs docker by absolute path with its folder first on PATH, where its credential helpers live.
func dockerCommand(ctx context.Context, docker string, env []string, args ...string) *exec.Cmd {
	bin, found := lookupDocker(docker)
	if !found {
		bin = docker
	}
	cmd := exec.CommandContext(ctx, bin, args...)
	cmd.Env = append(os.Environ(), env...)
	if found {
		cmd.Env = append(cmd.Env, "PATH="+prependPath(filepath.Dir(bin), envValue(cmd.Env, "PATH")))
	}
	return cmd
}

func prependPath(dir, path string) string {
	if path == "" {
		return dir
	}
	return dir + string(os.PathListSeparator) + path
}

// envValue is key's last value in env, as exec resolves duplicates; Windows names are case-insensitive.
func envValue(env []string, key string) string {
	value := ""
	for _, kv := range env {
		name, v, ok := strings.Cut(kv, "=")
		if ok && (name == key || runtime.GOOS == "windows" && strings.EqualFold(name, key)) {
			value = v
		}
	}
	return value
}

func containerName(worldID string) string { return "cinnabar-bds-" + worldID }

var pullLayer = regexp.MustCompile(`^([0-9a-f]{12}): (.+)$`)

// pullProgress counts image layers from `docker pull` output.
type pullProgress struct {
	layers map[string]bool // layer id -> complete
	done   int
}

func (pp *pullProgress) line(line string) (done, total int) {
	if match := pullLayer.FindStringSubmatch(strings.TrimSpace(line)); match != nil {
		complete := match[2] == "Pull complete" || match[2] == "Already exists"
		if was, seen := pp.layers[match[1]]; !seen || (complete && !was) {
			pp.layers[match[1]] = complete
			if complete {
				pp.done++
			}
		}
	}
	return pp.done, len(pp.layers)
}

// ensureImage pulls the image once, reporting layer progress.
func (r BDSRunner) ensureImage(ctx context.Context, image string) error {
	if err := r.dockerCmd(ctx, "image", "inspect", image).Run(); err == nil {
		return nil
	}
	p := r.Provisioner
	p.setOp(SetupPullingImage, "", 0, 0)
	cmd := r.dockerCmd(ctx, "pull", "--platform", "linux/amd64", image)
	out, err := cmd.StdoutPipe()
	if err != nil {
		return p.failWith("server image download failed", err)
	}
	var stderr strings.Builder
	cmd.Stderr = &stderr
	if err := cmd.Start(); err != nil {
		return p.failWith("server image download failed", fmt.Errorf("localworld: pull server image: %w", err))
	}
	progress := pullProgress{layers: map[string]bool{}}
	scanner := bufio.NewScanner(out)
	for scanner.Scan() {
		p.setLayers(progress.line(scanner.Text()))
	}
	if err := cmd.Wait(); err != nil {
		p.log().Error("docker pull failed", "output", strings.TrimSpace(stderr.String()))
		return p.failWith("server image download failed", fmt.Errorf("localworld: pull server image: %w", err))
	}
	p.setOp("", "", 0, 0)
	return nil
}

// containerArgs builds `docker run`: loopback-only HTTP and UDP mappings, the host-provisioned install as /data (the
// image skips its own download when bedrock_server-<version> is present), the world folder bind-mounted into
// the server's worlds directory, and the EULA acknowledged (callers check acceptance first).
func containerArgs(spec StartSpec, image, version, installDir string, hostPort, maxPlayers int, lanVisible bool, lanHostPort int) []string {
	w := spec.World
	levelType := "DEFAULT"
	if w.Generator == GeneratorFlat {
		levelType = "FLAT"
	}
	view := clampInt(orDefault(spec.Options.ViewDistance, defaultBDSView), 5, 32)
	args := []string{
		"run", "--rm", "--name", containerName(w.ID), "--platform", "linux/amd64",
		"-p", fmt.Sprintf("%s:%d:%d/tcp", localServerHost, hostPort, bdsContainerHTTPPort),
		"-p", fmt.Sprintf("%s:%s:%s/udp", localServerHost, udpPortRange(hostPort, maxPlayers), udpPortRange(bdsContainerUDPPort, maxPlayers)),
		"--health-cmd", fmt.Sprintf("curl --fail --silent http://%s:%d/v1/join > /dev/null", localServerHost, bdsContainerHTTPPort),
		"-v", installDir + ":/data",
		"-v", filepath.Join(spec.Dir, "db") + ":/data/worlds/" + w.ID,
	}
	if lanVisible {
		args = append(args, "-p", fmt.Sprintf("%s:%d:%d/udp", localServerHost, orDefault(lanHostPort, discovery.DefaultPort), discovery.DefaultPort))
	}
	for _, kv := range [][2]string{
		{"EULA", "TRUE"}, {"VERSION", version}, {"DIRECT_DOWNLOAD_URL", fmt.Sprintf(directURLFormat, "linux", version)},
		{"SERVER_NAME", sanitizeProperty(w.Name)},
		{"GAMEMODE", w.GameMode}, {"DIFFICULTY", w.Difficulty}, {"ALLOW_CHEATS", strconv.FormatBool(w.AllowCheats)},
		{"DEFAULT_PLAYER_PERMISSION_LEVEL", commandPermission(w.AllowCheats)},
		{"MAX_PLAYERS", strconv.Itoa(maxPlayers)}, {"ONLINE_MODE", "false"}, {"ALLOW_LIST", "false"},
		{"LEVEL_NAME", w.ID}, {"LEVEL_SEED", strconv.FormatInt(w.Seed, 10)}, {"LEVEL_TYPE", levelType},
		{"VIEW_DISTANCE", strconv.Itoa(view)}, {"TICK_DISTANCE", strconv.Itoa(clampInt(view, 4, 12))},
		{"PLAYER_IDLE_TIMEOUT", "0"},
		{"TRANSPORT", string(TransportNetherNetHTTP)}, {"SERVER_PORT", strconv.Itoa(bdsContainerHTTPPort)},
		{"SERVER_IP", "0.0.0.0"}, {"SERVER_UDP_PORTS", bdsUDPMapping(hostPort, bdsContainerUDPPort, maxPlayers)},
		{"ENABLE_LAN_VISIBILITY", strconv.FormatBool(lanVisible)},
		{"ENABLE_BDS_V6BIND_FIX", "TRUE"}, // the image's shim for IPv6 binds Docker cannot serve
	} {
		args = append(args, "-e", kv[0]+"="+kv[1])
	}
	return append(args, image)
}

// checkDocker is the open-time probe; a stopped daemon flips the runtime so the client offers Retry.
func (r BDSRunner) checkDocker(ctx context.Context) error {
	p := r.Provisioner
	p.setOp(SetupCheckingRuntime, "", 0, 0)
	probe, cancel := context.WithTimeout(ctx, 20*time.Second)
	defer cancel()
	if err := r.dockerCmd(probe, "info").Run(); err != nil {
		p.SetRuntime(RuntimeInfo{RuntimeNone, "Docker stopped; new worlds use dragonfly", "docker_not_running"})
		p.setOp("", "", 0, 0)
		return fmt.Errorf("%w: %w", ErrDockerNotRunning, ErrBackendUnavailable)
	}
	p.setOp("", "", 0, 0)
	return nil
}

// linkVersionedBinary names the server binary as the image expects so it never downloads its own copy.
func linkVersionedBinary(binary, version string) error {
	versioned := binary + "-" + version
	if _, err := os.Stat(versioned); err == nil {
		return nil
	}
	if err := os.Link(binary, versioned); err != nil {
		return fmt.Errorf("localworld: prepare server binary: %w", err)
	}
	return nil
}

func orDefault(v, fallback int) int {
	if v <= 0 {
		return fallback
	}
	return v
}

func (r BDSRunner) startContainer(ctx context.Context, spec StartSpec) (Instance, error) {
	p := r.Provisioner
	if !p.eulaAccepted() {
		return nil, ErrEULARequired
	}
	image, err := r.pinnedImage()
	if err != nil {
		return nil, err
	}
	log := r.Log
	if log == nil {
		log = slog.Default()
	}
	timeout := r.StartTimeout
	if timeout <= 0 {
		timeout = containerStartTimeout
	}
	maxPlayers := r.MaxPlayers
	if maxPlayers <= 0 {
		maxPlayers = 1
	}
	if err := r.checkDocker(ctx); err != nil {
		return nil, err
	}
	if err := r.ensureImage(ctx, image); err != nil {
		return nil, err
	}
	binary, err := p.Ensure(ctx)
	if err != nil {
		return nil, err
	}
	installDir := filepath.Dir(binary)
	version := filepath.Base(installDir)
	if err := linkVersionedBinary(binary, version); err != nil {
		return nil, err
	}
	if err := os.MkdirAll(filepath.Join(spec.Dir, "db"), 0o700); err != nil {
		return nil, fmt.Errorf("localworld: create world folder: %w", err)
	}
	address, err := freeBDSAddress(maxPlayers, r.HostPort)
	if err != nil {
		return nil, err
	}
	if r.LANVisible {
		if err := checkBDSLANPort(portOf(address), maxPlayers, orDefault(r.LANHostPort, discovery.DefaultPort)); err != nil {
			return nil, err
		}
	}
	name := containerName(spec.World.ID)
	_ = r.dockerCmd(ctx, "rm", "-f", name).Run() // a leftover from a crashed core
	cmd := r.dockerCmd(context.Background(), containerArgs(spec, image, version, installDir, portOf(address), maxPlayers, r.LANVisible, r.LANHostPort)...)
	inst, err := launch(ctx, launchSpec{
		cmd: cmd, address: address, log: log.With("component", "bds-container", "world", spec.World.ID), timeout: timeout,
		ready: func(line string) bool { return strings.Contains(line, bdsReadyMarker) },
	})
	if err != nil {
		_ = r.dockerCmd(context.Background(), "rm", "-f", name).Run()
		return nil, err
	}
	wrapped := &containerInstance{Instance: inst, runner: r, name: name, maxPlayers: maxPlayers}
	go func() {
		<-inst.Done()
		wrapped.cleanupOnce()
	}()
	return wrapped, nil
}

// containerInstance stops the container gracefully (BDS saves on SIGTERM) and always removes it.
type containerInstance struct {
	Instance
	runner     BDSRunner
	name       string
	once       sync.Once
	maxPlayers int
}

// MaxPlayers is the server's player limit, the host included.
func (c *containerInstance) MaxPlayers() int { return c.maxPlayers }

func (c *containerInstance) LANAddress() string {
	if !c.runner.LANVisible {
		return ""
	}
	return bdsLANAddress(c.runner.LANHostPort)
}

func (c *containerInstance) cleanupOnce() {
	c.once.Do(func() { _ = c.runner.dockerCmd(context.Background(), "rm", "-f", c.name).Run() })
}

func (c *containerInstance) Stop(ctx context.Context) error {
	_ = c.runner.dockerCmd(ctx, "stop", "-t", strconv.Itoa(containerStopSeconds), c.name).Run()
	err := c.Instance.Stop(ctx)
	c.cleanupOnce()
	return err
}

// CanPause is false for the same reason as native BDS.
func (c *containerInstance) CanPause() bool { return false }
