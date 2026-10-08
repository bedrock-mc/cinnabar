package main

import (
	"bytes"
	"context"
	"errors"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"slices"
	"strconv"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/df-mc/dragonfly/server/world"

	"github.com/hashimthearab/rust-mcbe/tools/localserver/experience"
	"github.com/hashimthearab/rust-mcbe/tools/localserver/extension"
)

func TestParseSettingsValidates(t *testing.T) {
	ok := []string{"-dir", "d", "-addr", "127.0.0.1:1", "-game-mode", "creative", "-difficulty", "hard"}
	s, err := parseSettings(ok, io.Discard)
	if err != nil || s.gameMode != "creative" || s.diff != "hard" {
		t.Fatalf("settings = %+v, %v", s, err)
	}
	for _, bad := range [][]string{
		{"-addr", "a"}, {"-dir", "d"},
		{"-dir", "d", "-addr", "a", "-game-mode", "hardcore"},
		{"-dir", "d", "-addr", "a", "-generator", "unknown"},
		{"-dir", "d", "-addr", "a", "-difficulty", "brutal"},
	} {
		if _, err := parseSettings(bad, io.Discard); err == nil {
			t.Fatalf("args %v must fail", bad)
		}
	}
}

func TestUserConfigIsOfflineAndScopedToDir(t *testing.T) {
	s := settings{dir: filepath.FromSlash("/w"), addr: "127.0.0.1:9", name: "n"}
	uc := s.userConfig()
	if uc.Server.AuthEnabled || uc.Network.Address != "127.0.0.1:9" || !uc.World.SaveData {
		t.Fatalf("config = %+v", uc)
	}
	for _, folder := range []string{uc.World.Folder, uc.Players.Folder, uc.Resources.Folder} {
		if !strings.HasPrefix(folder, s.dir) {
			t.Fatalf("folder %q escapes world dir", folder)
		}
	}
}

func TestServeCommandsProtocol(t *testing.T) {
	var mu sync.Mutex
	var calls []bool
	done := make(chan struct{})
	go func() {
		serveCommands(context.Background(), strings.NewReader("pause\nbogus\nresume\nstop\npause\n"), commands{pause: func(p bool) {
			mu.Lock()
			calls = append(calls, p)
			mu.Unlock()
		}})
		close(done)
	}()
	select {
	case <-done:
	case <-time.After(2 * time.Second):
		t.Fatal("serveCommands did not stop on stop")
	}
	mu.Lock()
	defer mu.Unlock()
	if len(calls) != 2 || !calls[0] || calls[1] {
		t.Fatalf("calls = %v", calls)
	}
}

func TestServeCommandsStopsOnEOF(t *testing.T) {
	done := make(chan struct{})
	go func() {
		serveCommands(context.Background(), strings.NewReader(""), commands{pause: func(bool) {}})
		close(done)
	}()
	select {
	case <-done:
	case <-time.After(2 * time.Second):
		t.Fatal("no stop on EOF")
	}
}

// Each well-formed reload line reloads its id; a reload line without exactly one id is ignored.
func TestReloadCommandParsed(t *testing.T) {
	var reloads []string
	in := "experience reload probe\nexperience reload\nexperience reload a b\n  experience   reload   other  \nstop\nexperience reload late\n"
	serveCommands(context.Background(), strings.NewReader(in), commands{
		pause:  func(bool) { t.Error("reload lines must not pause") },
		reload: func(id string) { reloads = append(reloads, id) },
	})
	if want := []string{"probe", "other"}; !slices.Equal(reloads, want) {
		t.Fatalf("reloads = %q, want %q", reloads, want)
	}
}

func TestExperiencesRequireRuntimeFlag(t *testing.T) {
	base := []string{"-dir", "d", "-addr", "127.0.0.1:1", "-experiences", "e"}
	if _, err := parseSettings(base, io.Discard); err == nil || !strings.Contains(err.Error(), "-experience-runtime") {
		t.Fatalf("-experiences without -experience-runtime: err = %v, want one naming -experience-runtime", err)
	}
	s, err := parseSettings(append(base, "-experience-runtime", "r"), io.Discard)
	if err != nil || s.experiences != "e" || s.runtime != "r" {
		t.Fatalf("settings = %+v, %v", s, err)
	}
}

// Without -experiences or the -extension flags the server starts as before and touches no
// Experience or client part data.
func TestNoFlagLeavesStartupUnchanged(t *testing.T) {
	dir := t.TempDir()
	var stdout bytes.Buffer
	if err := run([]string{"-dir", dir, "-addr", "127.0.0.1:0"}, strings.NewReader("stop\n"), &stdout, io.Discard); err != nil {
		t.Fatalf("run: %v", err)
	}
	if stdout.String() != "ready\n" {
		t.Fatalf("stdout = %q, want %q", stdout.String(), "ready\n")
	}
	if _, err := os.Stat(filepath.Join(dir, experienceDataDir)); !errors.Is(err, fs.ErrNotExist) {
		t.Fatalf("%s exists without -experiences: %v", experienceDataDir, err)
	}
	for _, path := range []string{filepath.Join(dir, extension.RevisionFile), markerPack(dir)} {
		if _, err := os.Stat(path); !errors.Is(err, fs.ErrNotExist) {
			t.Fatalf("%s exists without the -extension flags: %v", path, err)
		}
	}
}

// An installed id that no artifact provides fails startup naming it, before "ready".
func TestMissingInstalledExperienceFailsStartup(t *testing.T) {
	dir := t.TempDir()
	store, err := experience.OpenStore(filepath.Join(dir, experienceDataDir))
	if err != nil {
		t.Fatal(err)
	}
	if err := store.SetInstalled([]experience.Loaded{{ID: "gone"}}); err != nil {
		t.Fatal(err)
	}
	args := []string{
		"-dir", dir, "-addr", "127.0.0.1:0",
		"-experiences", t.TempDir(), "-experience-runtime", filepath.Join(dir, "no-runtime"),
	}
	var stdout bytes.Buffer
	err = run(args, strings.NewReader(""), &stdout, io.Discard)
	if err == nil || !strings.Contains(err.Error(), `"gone"`) {
		t.Fatalf("run: err = %v, want one naming \"gone\"", err)
	}
	if stdout.Len() != 0 {
		t.Fatalf("stdout = %q, want nothing", stdout.String())
	}
}

// Pause must suspend every dimension and resume must keep each world's own settings.
func TestSetPausedSuspendsAndRestoresWorlds(t *testing.T) {
	cycling, stopped := world.Config{}.New(), world.Config{}.New()
	defer cycling.Close()
	defer stopped.Close()
	stopped.StopTime()
	worlds := []*world.World{cycling, stopped}

	setPaused(worlds, true)
	for _, w := range worlds {
		if !w.Paused() {
			t.Fatal("world not paused")
		}
	}
	setPaused(worlds, false)
	if cycling.Paused() || stopped.Paused() {
		t.Fatal("world still paused after resume")
	}
	if !cycling.TimeCycle() || stopped.TimeCycle() {
		t.Fatalf("resume changed time cycle: cycling=%v stopped=%v", cycling.TimeCycle(), stopped.TimeCycle())
	}
}

// Saved worlds override the capture server's permissive command default explicitly.
func TestManagedWorldCommandPermission(t *testing.T) {
	for _, allowed := range []bool{false, true} {
		args := []string{"-dir", t.TempDir(), "-addr", "127.0.0.1:19132", "-allow-cheats=" + strconv.FormatBool(allowed)}
		cfg, err := parseSettings(args, io.Discard)
		if err != nil || cfg.allowCheats != allowed {
			t.Fatalf("command permission = %v, %v", cfg.allowCheats, err)
		}
	}
}
