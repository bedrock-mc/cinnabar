package main

import (
	"context"
	"fmt"
	"log/slog"
	"os"
	"path/filepath"
	"runtime"

	"github.com/hashimthearab/rust-mcbe/core/control"
	"github.com/hashimthearab/rust-mcbe/core/localworld"
	"github.com/hashimthearab/rust-mcbe/core/proxy"
)

const localServerName = "bedrock-local-server"

func defaultLocalServerBinary() (string, error) {
	exe, err := os.Executable()
	if err != nil {
		return "", err
	}
	name := localServerName
	if runtime.GOOS == "windows" {
		name += ".exe"
	}
	return filepath.Join(filepath.Dir(exe), name), nil
}

// detectRuntime probes how BDS can run; -local-backend=dragonfly skips the probe.
func detectRuntime(ctx context.Context, opts options) localworld.RuntimeInfo {
	if opts.localBackend == "dragonfly" {
		return localworld.RuntimeInfo{Kind: localworld.RuntimeNone, Reason: "dragonfly forced by -local-backend"}
	}
	return localworld.DetectRuntime(ctx, opts.docker)
}

// startupRuntime is the runtime known without waiting on Docker; pending means a background probe must settle it.
func startupRuntime(opts options) (info localworld.RuntimeInfo, pending bool) {
	if opts.localBackend == "dragonfly" || localworld.PlatformSupportsBDS() {
		return detectRuntime(context.Background(), opts), false
	}
	return localworld.RuntimeInfo{Kind: localworld.RuntimeContainer, Reason: "checking Docker"}, true
}

// defaultBackend resolves -local-backend; auto picks BDS when it can run natively or in a container.
func defaultBackend(flag string, info localworld.RuntimeInfo) string {
	if flag == "auto" || flag == "" {
		return localworld.DefaultBackend(info)
	}
	return flag
}

// openLocalWorlds builds the manager with a dragonfly runner (when its binary exists) and a BDS runner.
// The dragonfly binary is required, and its absence fatal, only when it is the default backend at startup;
// otherwise Dragonfly worlds are refused.
func openLocalWorlds(opts options, logger *slog.Logger) (*localworld.Manager, error) {
	runtimeInfo, pending := startupRuntime(opts)
	backend := defaultBackend(opts.localBackend, runtimeInfo)
	binary := opts.localServerBin
	if binary == "" {
		var err error
		if binary, err = defaultLocalServerBinary(); err != nil {
			return nil, fmt.Errorf("locate local world server: %w", err)
		}
	}
	runners := localworld.Runners{}
	var missingDragonfly error
	if info, err := os.Stat(binary); err == nil && !info.IsDir() {
		runners[localworld.BackendDragonfly] = localworld.ProcessRunner{Binary: binary, Log: logger}
	} else {
		missingDragonfly = fmt.Errorf("local world server binary not found at %s; build it with `make local-server`", binary)
		if backend == localworld.BackendDragonfly {
			return nil, missingDragonfly
		}
	}
	store, err := localworld.OpenStore(opts.localWorldsDir)
	if err != nil {
		return nil, err
	}
	store.SetDefaultBackend(backend)
	bdsDir := opts.bdsDir
	if bdsDir == "" {
		bdsDir = filepath.Join(filepath.Dir(filepath.Clean(opts.localWorldsDir)), "bds")
	}
	provisioner := &localworld.Provisioner{Root: bdsDir, Version: opts.bdsVersion, Log: logger}
	provisioner.SetRuntime(runtimeInfo)
	provisioner.SetDetector(func(ctx context.Context) localworld.RuntimeInfo { return detectRuntime(ctx, opts) })
	maxPlayers := opts.bdsMaxPlayers
	if maxPlayers == 0 {
		// Room for friends joining the hosted world, as vanilla allows.
		maxPlayers = proxy.FriendWorldMaxPlayers
	}
	runners[localworld.BackendBDS] = localworld.BDSRunner{
		Provisioner: provisioner, Log: logger, Docker: opts.docker, Image: opts.bdsImage,
		MaxPlayers: maxPlayers, HostPort: opts.bdsHostPort, LANVisible: opts.bdsLANVisible, LANHostPort: opts.bdsLANHostPort,
	}
	manager := localworld.NewManager(store, runners, logger)
	if missingDragonfly != nil {
		// Docker detection may still settle on Dragonfly; its worlds are then refused, not saved unopenable.
		manager.SetUnavailable(localworld.BackendDragonfly, missingDragonfly)
	}
	manager.SetSetup(provisioner)
	manager.SetAutoBackend(opts.localBackend == "auto" || opts.localBackend == "")
	logger.Info("local worlds enabled", "dir", opts.localWorldsDir, "default_backend", backend, "bds_runtime", runtimeInfo.Kind, "reason", runtimeInfo.Reason)
	if pending {
		provisioner.DetectInBackground(runtimeInfo)
	}
	return manager, nil
}

// worldSentinels are the local-world failures the control plane reports by code.
var worldSentinels = []sentinelPair{
	{control.ErrWorldNotFound, localworld.ErrNotFound},
	{control.ErrWorldInvalid, localworld.ErrInvalid},
	{control.ErrWorldBusy, localworld.ErrBusy},
	{control.ErrWorldInUse, localworld.ErrInUse},
	{control.ErrWorldNotOpen, localworld.ErrNotOpen},
	{control.ErrWorldRuntimePending, localworld.ErrRuntimePending},
	{control.ErrEULARequired, localworld.ErrEULARequired},
	{control.ErrWorldBackendUnavailable, localworld.ErrBackendUnavailable},
}

func worldErr(err error) error { return classify(err, &worldSentinels) }

// controlWorlds serves the control plane's world methods from the local-world manager.
type controlWorlds struct {
	manager *localworld.Manager
	onOpen  func()                              // runs after each successful Open
	invite  func(context.Context, string) error // nil while friends cannot be invited
}

func (w controlWorlds) List() ([]control.World, error) {
	worlds, err := w.manager.List()
	return mapSlice(worlds, func(world localworld.World) control.World { return control.World(world) }), worldErr(err)
}

func (w controlWorlds) Open(id string, viewDistance int) error {
	err := w.manager.Open(id, localworld.OpenOptions{ViewDistance: viewDistance})
	if err == nil && w.onOpen != nil {
		w.onOpen()
	}
	return worldErr(err)
}

func (w controlWorlds) Close() error { return worldErr(w.manager.Close()) }

func (w controlWorlds) Status() control.WorldStatus { return wireWorldStatus(w.manager.Status()) }

func wireWorldStatus(status localworld.Status) control.WorldStatus {
	wire := control.WorldStatus{
		State: string(status.State), WorldID: status.WorldID, Backend: status.Backend, Paused: status.Paused,
		PauseSupported: status.PauseSupported, Error: status.Error,
		BackendUnavailableReason: status.BackendUnavailableReason, MaxPlayers: status.MaxPlayers,
	}
	if setup := status.Setup; setup != nil {
		wire.Setup = &control.WorldSetup{
			State: string(setup.State), Version: setup.Version, BytesDone: setup.BytesDone, BytesTotal: setup.BytesTotal,
			LayersDone: setup.LayersDone, LayersTotal: setup.LayersTotal, EULAAccepted: setup.EULAAccepted,
			Error: setup.Error, Runtime: setup.Runtime, Reason: setup.Reason, UnavailableReason: setup.UnavailableReason,
		}
	}
	return wire
}

func (w controlWorlds) Create(spec control.WorldSpec) (control.World, error) {
	world, err := w.manager.Create(localworld.Spec(spec))
	return control.World(world), worldErr(err)
}

func (w controlWorlds) Update(id string, update control.WorldUpdate) (control.World, error) {
	world, err := w.manager.Update(id, localworld.Update(update))
	return control.World(world), worldErr(err)
}

func (w controlWorlds) Delete(id string) error { return worldErr(w.manager.Delete(id)) }

func (w controlWorlds) SetPaused(paused bool) error { return worldErr(w.manager.SetPaused(paused)) }

func (w controlWorlds) AcceptEULA() error { return worldErr(w.manager.AcceptEULA()) }

func (w controlWorlds) Prefs(ctx context.Context, update control.WorldPrefsUpdate) (control.WorldPrefs, error) {
	prefs, err := w.manager.Prefs(ctx, localworld.PrefsUpdate(update))
	return control.WorldPrefs(prefs), worldErr(err)
}

func (w controlWorlds) Invite(ctx context.Context, xuid string) error {
	if w.invite == nil {
		return control.ErrWorldNotOpen
	}
	return worldErr(w.invite(ctx, xuid))
}
