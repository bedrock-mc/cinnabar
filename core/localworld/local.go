// Package localworld holds the launcher's open local world, which the launcher runs and reports
// over local_world.v1, for the proxy's routing and friend hosting.
package localworld

import (
	"context"
	"errors"
	"fmt"
	"sync"
)

var (
	ErrInvalid = errors.New("invalid local world report")
	ErrNotOpen = errors.New("no world is open")
)

const (
	GameModeSurvival  = "survival"
	GameModeCreative  = "creative"
	GameModeAdventure = "adventure"

	BackendBDS       = "bds"       // Bedrock Dedicated Server: vanilla worldgen and mobs
	BackendDragonfly = "dragonfly" // built-in server
)

// State is the lifecycle of the launcher's open world.
type State string

const (
	StateIdle     State = "idle"
	StateStarting State = "starting"
	StateRunning  State = "running"
	StateStopping State = "stopping"
	StateFailed   State = "failed" // stays until closed so a failed open never falls through to another target
)

// World identifies the open world to Xbox friends.
type World struct {
	ID       string `json:"id"`
	Name     string `json:"name"`
	GameMode string `json:"game_mode"`
	Backend  string `json:"backend"`
}

// Status is the launcher's report of its open world; Error never carries paths.
type Status struct {
	State  State             `json:"state"`
	World  *World            `json:"world,omitempty"`  // set unless idle
	Target *ConnectionTarget `json:"target,omitempty"` // set while running
	Error  string            `json:"error,omitempty"`
}

// Validate rejects reports the proxy could not route.
func (s Status) Validate() error {
	switch s.State {
	case StateIdle:
		return nil
	case StateStarting, StateStopping, StateFailed:
	case StateRunning:
		if s.Target == nil || s.Target.Address == "" {
			return fmt.Errorf("%w: a running world needs a target", ErrInvalid)
		}
		switch s.Target.Transport {
		case TransportRakNet, TransportNetherNetHTTP, TransportNetherNetLAN:
		default:
			return fmt.Errorf("%w: unknown transport %q", ErrInvalid, s.Target.Transport)
		}
	default:
		return fmt.Errorf("%w: unknown state %q", ErrInvalid, s.State)
	}
	if s.World == nil || s.World.ID == "" {
		return fmt.Errorf("%w: an open world needs an id", ErrInvalid)
	}
	return nil
}

func (s Status) open() bool { return s.State == StateStarting || s.State == StateRunning }

// Local holds the launcher's latest report for the proxy and friend hosting.
type Local struct {
	mu      sync.Mutex
	status  Status
	changed chan struct{} // closed and replaced on every report
}

func NewLocal() *Local {
	return &Local{status: Status{State: StateIdle}, changed: make(chan struct{})}
}

// SetLocal records a report; opened is true when a world began opening, which supersedes any other route.
func (l *Local) SetLocal(status Status) (opened bool, err error) {
	if err := status.Validate(); err != nil {
		return false, err
	}
	l.mu.Lock()
	defer l.mu.Unlock()
	opened = status.open() && !(l.status.open() && l.status.World.ID == status.World.ID)
	l.status = status
	close(l.changed)
	l.changed = make(chan struct{})
	return opened, nil
}

// Running reports the open world while its server runs, and a channel closed on the next report.
func (l *Local) Running() (world World, running bool, changed <-chan struct{}) {
	l.mu.Lock()
	defer l.mu.Unlock()
	running = l.status.State == StateRunning
	if running {
		world = *l.status.World
	}
	return world, running, l.changed
}

// ConnectionTarget reports the running world's address and transport, waiting out a start; ok is
// false when no world is open.
func (l *Local) ConnectionTarget(ctx context.Context) (ConnectionTarget, bool, error) {
	for {
		l.mu.Lock()
		status, changed := l.status, l.changed
		l.mu.Unlock()
		switch status.State {
		case StateIdle:
			return ConnectionTarget{}, false, nil
		case StateRunning:
			return *status.Target, true, nil
		case StateFailed:
			return ConnectionTarget{}, false, fmt.Errorf("local world unavailable: %s", status.Error)
		case StateStopping:
			return ConnectionTarget{}, false, errors.New("local world is closing")
		}
		select {
		case <-changed:
		case <-ctx.Done():
			return ConnectionTarget{}, false, ctx.Err()
		}
	}
}
