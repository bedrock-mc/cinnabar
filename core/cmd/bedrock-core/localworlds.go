package main

import (
	"context"
	"fmt"
	"log/slog"
	"os"
	"path/filepath"
	"runtime"

	"github.com/hashimthearab/rust-mcbe/core/localworld"
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

// defaultBackend resolves the operator default independently of BDS availability.
func defaultBackend(flag string) string {
	if flag == "auto" || flag == "" {
		return localworld.BackendDragonfly
	}
	return flag
}

// openLocalWorlds builds the manager with a dragonfly runner (when its binary exists) and a BDS runner.
// The dragonfly binary is required, and its absence fatal, only when it is the default backend at startup;
// otherwise Dragonfly worlds are refused.
func openLocalWorlds(opts options, logger *slog.Logger) (*localworld.Manager, error) {
	runtimeInfo, pending := startupRuntime(opts)
	backend := defaultBackend(opts.localBackend)
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
	runners[localworld.BackendBDS] = localworld.BDSRunner{
		Provisioner: provisioner, Log: logger, Docker: opts.docker, Image: opts.bdsImage,
		MaxPlayers: opts.bdsMaxPlayers, HostPort: opts.bdsHostPort, LANVisible: opts.bdsLANVisible, LANHostPort: opts.bdsLANHostPort,
	}
	manager := localworld.NewManager(store, runners, logger)
	if missingDragonfly != nil {
		// Explicit Dragonfly requests are refused when its executable is missing.
		manager.SetUnavailable(localworld.BackendDragonfly, missingDragonfly)
	}
	manager.SetSetup(provisioner)
	logger.Info("local worlds enabled", "dir", opts.localWorldsDir, "default_backend", backend, "bds_runtime", runtimeInfo.Kind, "reason", runtimeInfo.Reason)
	if pending {
		provisioner.DetectInBackground(runtimeInfo)
	}
	return manager, nil
}
