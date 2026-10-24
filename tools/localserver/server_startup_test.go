package main

import (
	"errors"
	"log/slog"
	"testing"

	"github.com/cinnabar-mc/cinnabar/tools/localserver/experience"

	"github.com/df-mc/dragonfly/server"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/session"
	"github.com/df-mc/dragonfly/server/world"
)

// startupListener records release without accepting a network connection.
type startupListener struct {
	server.Listener
	closed bool
}

// Close records that startup cleanup released the listener.
func (l *startupListener) Close() error { l.closed = true; return nil }

// Accept rejects any attempt to admit players before world configuration succeeds.
func (l *startupListener) Accept() (session.Conn, error) { return nil, errors.New("unexpected accept") }

// startupPlayers tracks the player provider opened by server configuration.
type startupPlayers struct {
	player.NopProvider
	closed bool
}

// Close records that startup cleanup released the player provider.
func (p *startupPlayers) Close() error { p.closed = true; return nil }

// TestServerCleanupBeforeListen preserves world setup errors and flushes the host without admission.
func TestServerCleanupBeforeListen(t *testing.T) {
	listener := &startupListener{}
	players := &startupPlayers{}
	srv, closeUnstarted := newLocalServer(server.Config{
		Log: slog.New(slog.DiscardHandler), PlayerProvider: players,
		Listeners: []func(server.Config) (server.Listener, error){func(server.Config) (server.Listener, error) { return listener, nil }},
	})
	dir := t.TempDir()
	store, err := experience.OpenStore(dir)
	if err != nil {
		t.Fatal(err)
	}
	host := experience.NewHost(nil, store, nil, "world", nil, slog.New(slog.DiscardHandler))
	t.Cleanup(func() { _ = host.Close() })
	key := experience.Key{Instance: experience.OverworldInstance}
	store.Place("probe", key)
	if err := store.SetData("probe", key, []byte("final"), true); err != nil {
		t.Fatal(err)
	}
	if err := host.BindDefaultWorlds(srv.World(), srv.Nether(), srv.End()); err != nil {
		t.Fatal(err)
	}
	if err := host.ConfigureWorlds(t.Context(), t.TempDir()); err != nil {
		t.Fatal(err)
	}
	setupErr := host.ConfigureWorlds(t.Context(), t.TempDir())
	if setupErr == nil {
		t.Fatal("repeated setup did not fail")
	}
	if err := errors.Join(setupErr, closeUnstarted(), host.Close()); !errors.Is(err, setupErr) {
		t.Fatalf("lost world setup error: %v", err)
	}
	reopened, err := experience.OpenStore(dir)
	if err != nil {
		t.Fatal(err)
	}
	if data, ok := reopened.Data("probe", key); !ok || string(data) != "final" {
		t.Fatal("host cleanup did not flush final state")
	}
	for _, w := range []*world.World{srv.World(), srv.Nether(), srv.End()} {
		if err := w.Do(func(*world.Tx) { t.Error("closed world accepted work") }).Wait(t.Context()); !errors.Is(err, world.ErrWorldClosed) {
			t.Errorf("world not closed: %v", err)
		}
	}
	if !listener.closed || !players.closed {
		t.Fatal("startup resources were not released")
	}
}
