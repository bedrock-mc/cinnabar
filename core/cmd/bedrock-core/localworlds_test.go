package main

import (
	"errors"
	"io"
	"strconv"
	"strings"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/localworld"
)

func TestLocalWorldsFlagsRequireControlStatus(t *testing.T) {
	if _, err := parseFlags([]string{"-local-worlds-dir", "w"}, io.Discard); err == nil || !strings.Contains(err.Error(), "-control-status") {
		t.Fatalf("err = %v", err)
	}
	if _, err := parseFlags([]string{"-local-server-bin", "b"}, io.Discard); err == nil {
		t.Fatal("local-server-bin without worlds dir must fail")
	}
	opts, err := parseFlags([]string{"-control-status", "-local-worlds-dir", "w", "-local-server-bin", "b"}, io.Discard)
	if err != nil || opts.localWorldsDir != "w" || opts.localServerBin != "b" {
		t.Fatalf("opts = %+v, %v", opts, err)
	}
}

func TestOpenLocalWorldsFailsClosedWhenBinaryMissing(t *testing.T) {
	_, err := openLocalWorlds(options{localWorldsDir: t.TempDir(), localServerBin: "/nonexistent/bedrock-local-server", localBackend: "dragonfly"}, newLifecycleLogger(io.Discard))
	if err == nil || !strings.Contains(err.Error(), "make local-server") {
		t.Fatalf("err = %v", err)
	}
}

func TestLocalBackendFlagIsValidated(t *testing.T) {
	if _, err := parseFlags([]string{"-local-backend", "java"}, io.Discard); err == nil {
		t.Fatal("unknown backend must fail")
	}
	opts, err := parseFlags(nil, io.Discard)
	if err != nil || opts.localBackend != "auto" {
		t.Fatalf("opts = %+v, %v", opts, err)
	}
}

func TestBDSMaxPlayersFlag(t *testing.T) {
	t.Parallel()
	for _, test := range []struct {
		name     string
		args     []string
		expected int
		wantErr  bool
	}{
		{name: "runner default", args: []string{}},
		{name: "explicit runner default", args: []string{"-bds-max-players", "0"}},
		{name: "shared comparison world", args: []string{"-bds-max-players", "8"}, expected: 8},
		{name: "negative limit", args: []string{"-bds-max-players", "-1"}, wantErr: true},
		{name: "invalid integer", args: []string{"-bds-max-players", "many"}, wantErr: true},
	} {
		t.Run(test.name, func(t *testing.T) {
			t.Parallel()
			opts, err := parseFlags(test.args, io.Discard)
			if (err != nil) != test.wantErr {
				t.Fatalf("parseFlags(%v) error = %v, want error %v", test.args, err, test.wantErr)
			}
			if err == nil && opts.bdsMaxPlayers != test.expected {
				t.Fatalf("bds max players = %d, want %d", opts.bdsMaxPlayers, test.expected)
			}
		})
	}
}

func TestBDSHostPortFlag(t *testing.T) {
	t.Parallel()
	for _, test := range []struct {
		name    string
		args    []string
		want    int
		wantErr bool
	}{
		{name: "dynamic default"},
		{name: "explicit dynamic", args: []string{"-bds-host-port", "0"}},
		{name: "conventional comparison port", args: []string{"-bds-host-port", strconv.Itoa(localworld.DefaultBDSPort)}, want: localworld.DefaultBDSPort},
		{name: "negative", args: []string{"-bds-host-port", "-1"}, wantErr: true},
		{name: "overflow", args: []string{"-bds-host-port", strconv.Itoa(1 << 16)}, wantErr: true},
		{name: "non-numeric", args: []string{"-bds-host-port", "default"}, wantErr: true},
	} {
		t.Run(test.name, func(t *testing.T) {
			t.Parallel()
			opts, err := parseFlags(test.args, io.Discard)
			if (err != nil) != test.wantErr || (err == nil && opts.bdsHostPort != test.want) {
				t.Fatalf("host port = %d, %v; want %d, error %v", opts.bdsHostPort, err, test.want, test.wantErr)
			}
		})
	}
}

func TestBDSLANVisibilityFlagIsExplicit(t *testing.T) {
	t.Parallel()
	for _, test := range []struct {
		args []string
		want bool
	}{
		{},
		{args: []string{"-bds-lan-visible"}, want: true},
		{args: []string{"-bds-lan-visible=false"}},
	} {
		opts, err := parseFlags(test.args, io.Discard)
		if err != nil || opts.bdsLANVisible != test.want {
			t.Fatalf("LAN visibility = %v, %v; want %v", opts.bdsLANVisible, err, test.want)
		}
	}
}

func TestBDSLANHostPortFlag(t *testing.T) {
	t.Parallel()
	for _, test := range []struct {
		arg     string
		want    int
		wantErr bool
	}{
		{arg: "0"},
		{arg: "5000", want: 5000},
		{arg: "-1", wantErr: true},
		{arg: strconv.Itoa(1 << 16), wantErr: true},
	} {
		opts, err := parseFlags([]string{"-bds-lan-host-port", test.arg}, io.Discard)
		if (err != nil) != test.wantErr || (err == nil && opts.bdsLANHostPort != test.want) {
			t.Fatalf("LAN host port = %d, %v; want %d, error %v", opts.bdsLANHostPort, err, test.want, test.wantErr)
		}
	}
}

func TestBDSDefaultDoesNotRequireDragonflyBinary(t *testing.T) {
	manager, err := openLocalWorlds(options{localWorldsDir: t.TempDir(), localServerBin: "/nonexistent/x", localBackend: "bds"}, newLifecycleLogger(io.Discard))
	if err != nil || manager == nil {
		t.Fatalf("manager = %v, err = %v", manager, err)
	}
	// Without the binary a Dragonfly world could never open, so it is refused rather than saved.
	if _, err := manager.Create(localworld.Spec{Name: "flat", Generator: localworld.GeneratorFlat, Backend: localworld.BackendDragonfly}); !errors.Is(err, localworld.ErrBackendUnavailable) {
		t.Fatalf("create without binary: %v", err)
	}
	if worlds, _ := manager.List(); len(worlds) != 0 {
		t.Fatalf("saved %v", worlds)
	}
}
