package localworld

import (
	"bytes"
	"context"
	"fmt"
	"log/slog"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
	"time"
)

const (
	bdsStartTimeout = 5 * time.Minute // first start generates spawn chunks
	bdsReadyMarker  = "Server started."
	defaultBDSView  = 10
)

// BDSRunner hosts each world in the official Bedrock Dedicated Server.
//
// BDS's NetherNet HTTP listener binds to loopback with online-mode off; its UDP window
// is sized to MaxPlayers. The world folder is exposed to it by a directory link
// at worlds/<id> inside the install, so only one world runs at a time.
type BDSRunner struct {
	Provisioner  *Provisioner
	Env          []string
	Log          *slog.Logger
	StartTimeout time.Duration // default 5m
	MaxPlayers   int           // default 1
	HostPort     int           // zero selects an available loopback TCP/UDP window
	LANVisible   bool          // enable BDS's documented LAN discovery; false by default
	LANHostPort  int           // container-only discovery host port; zero uses the pinned discovery port

	// Container runtime (macOS): the Linux build runs in Docker.
	Docker string // default "docker"
	Image  string // must name a digest (the target manifest's bds_container_image)
}

func (r BDSRunner) Start(ctx context.Context, spec StartSpec) (Instance, error) {
	if r.Provisioner == nil {
		return nil, ErrBackendUnavailable
	}
	if kind, _ := r.Provisioner.runtimeKind(); kind == RuntimeContainer {
		return r.startContainer(ctx, spec)
	}
	binary, err := r.Provisioner.Ensure(ctx)
	if err != nil {
		return nil, err
	}
	log := r.Log
	if log == nil {
		log = slog.Default()
	}
	timeout := r.StartTimeout
	if timeout <= 0 {
		timeout = bdsStartTimeout
	}
	maxPlayers := r.MaxPlayers
	if maxPlayers <= 0 {
		maxPlayers = 1
	}
	address, err := freeBDSAddress(maxPlayers, r.HostPort)
	if err != nil {
		return nil, err
	}
	installDir := filepath.Dir(binary)
	worldDir := filepath.Join(spec.Dir, "db")
	if err := os.MkdirAll(worldDir, 0o700); err != nil {
		return nil, fmt.Errorf("localworld: create world folder: %w", err)
	}
	link := filepath.Join(installDir, "worlds", spec.World.ID)
	if err := linkWorld(link, worldDir); err != nil {
		return nil, err
	}
	props := serverProperties(spec, portOf(address), maxPlayers, r.LANVisible)
	if err := os.WriteFile(filepath.Join(installDir, "server.properties"), props, 0o600); err != nil {
		unlinkWorld(link)
		return nil, fmt.Errorf("localworld: write server.properties: %w", err)
	}
	cmd := exec.Command(binary)
	cmd.Dir = installDir
	cmd.Env = append(os.Environ(), "LD_LIBRARY_PATH=.")
	cmd.Env = append(cmd.Env, r.Env...)
	inst, err := launch(ctx, launchSpec{
		cmd: cmd, address: address, log: log.With("component", "bds", "world", spec.World.ID), timeout: timeout,
		canPause: false,
		ready:    func(line string) bool { return strings.Contains(line, bdsReadyMarker) },
	})
	if err != nil {
		unlinkWorld(link)
		return nil, err
	}
	wrapped := &bdsInstance{Instance: inst, cleanup: func() { unlinkWorld(link) }, maxPlayers: maxPlayers}
	if r.LANVisible {
		wrapped.lanAddress = bdsLANAddress(0)
	}
	go func() {
		<-inst.Done()
		wrapped.cleanupOnce()
	}()
	return wrapped, nil
}

// bdsInstance drops the world link once the server has exited.
type bdsInstance struct {
	Instance
	cleanup    func()
	once       sync.Once
	lanAddress string
	maxPlayers int
}

func (b *bdsInstance) LANAddress() string { return b.lanAddress }

// MaxPlayers is the server's player limit, the host included.
func (b *bdsInstance) MaxPlayers() int { return b.maxPlayers }

func (b *bdsInstance) cleanupOnce() { b.once.Do(b.cleanup) }

func (b *bdsInstance) Stop(ctx context.Context) error {
	err := b.Instance.Stop(ctx)
	b.cleanupOnce()
	return err
}

// CanPause is false: BDS does not register /globalpause (the integrated server's sim-time pause),
// gamerules leave mobs and redstone running, and SIGSTOP would also freeze the connection.
func (b *bdsInstance) CanPause() bool { return false }

func portOf(address string) int {
	port, _ := strconv.Atoi(address[strings.LastIndexByte(address, ':')+1:])
	return port
}

func clampInt(v, lo, hi int) int {
	return max(lo, min(hi, v))
}

// serverProperties renders the per-launch BDS configuration. Seed, level type and the initial
// settings only apply when the world is first created; gamemode and difficulty apply on every start.
func serverProperties(spec StartSpec, port, maxPlayers int, lanVisible bool) []byte {
	w := spec.World
	levelType := "DEFAULT"
	if w.Generator == GeneratorFlat {
		levelType = "FLAT"
	}
	view := spec.Options.ViewDistance
	if view <= 0 {
		view = defaultBDSView
	}
	view = clampInt(view, 5, 32)
	var b bytes.Buffer
	for _, kv := range [][2]string{
		{"server-name", sanitizeProperty(w.Name)},
		{"gamemode", w.GameMode},
		{"force-gamemode", "false"},
		{"difficulty", w.Difficulty},
		{"allow-cheats", strconv.FormatBool(w.AllowCheats)},
		{"default-player-permission-level", commandPermission(w.AllowCheats)},
		{"max-players", strconv.Itoa(maxPlayers)},
		{"online-mode", "false"},
		{"allow-list", "false"},
		{"server-port", strconv.Itoa(port)},
		{"server-ip", localServerHost},
		{"server-udp-ports", bdsUDPMapping(port, port, maxPlayers)},
		{"level-name", w.ID},
		{"level-seed", strconv.FormatInt(w.Seed, 10)},
		{"level-type", levelType},
		{"view-distance", strconv.Itoa(view)},
		{"tick-distance", strconv.Itoa(clampInt(view, 4, 12))},
		{"player-idle-timeout", "0"},
		{"transport", string(TransportNetherNetHTTP)},
		{"enable-lan-visibility", strconv.FormatBool(lanVisible)},
		{"texturepack-required", "false"},
		{"content-log-file-enabled", "false"},
	} {
		fmt.Fprintf(&b, "%s=%s\n", kv[0], kv[1])
	}
	return b.Bytes()
}

func sanitizeProperty(value string) string {
	return strings.Map(func(r rune) rune {
		if r < ' ' || r == 0x7f || r == '#' {
			return -1
		}
		return r
	}, value)
}

// linkWorld exposes target at link (a junction on Windows, which needs no privilege; a symlink elsewhere).
func linkWorld(link, target string) error {
	if err := os.MkdirAll(filepath.Dir(link), 0o700); err != nil {
		return fmt.Errorf("localworld: prepare worlds folder: %w", err)
	}
	unlinkWorld(link)
	if err := createWorldLink(link, target); err != nil {
		return fmt.Errorf("localworld: link world folder: %w", err)
	}
	return nil
}

// unlinkWorld removes only the link, never the world data it points at.
func unlinkWorld(link string) {
	if info, err := os.Lstat(link); err == nil && (info.Mode()&(os.ModeSymlink|os.ModeIrregular) != 0 || info.IsDir()) {
		_ = os.Remove(link)
	}
}

const (
	permissionMember   = "member"
	permissionOperator = "operator"
)

// commandPermission gives a local world host command access only when cheats are enabled.
func commandPermission(allowCheats bool) string {
	if allowCheats {
		return permissionOperator
	}
	return permissionMember
}
