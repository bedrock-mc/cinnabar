// Package localworld holds the launcher's open local world, which the launcher runs and reports
// over local_world.v1, for the proxy's routing and friend hosting.
package localworld

import (
	"context"
	"errors"
	"fmt"
	"sync"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
)

var (
	ErrInvalid = errors.New("invalid local world report")
	ErrNotOpen = errors.New("no world is open")
)

const (
	GameModeSurvival  = bridgecontract.GameModeSurvival
	GameModeCreative  = bridgecontract.GameModeCreative
	GameModeAdventure = bridgecontract.GameModeAdventure

	BackendBDS       = bridgecontract.BackendBds       // Bedrock Dedicated Server: vanilla worldgen and mobs
	BackendDragonfly = bridgecontract.BackendDragonfly // built-in server
)

// State is the lifecycle of the launcher's open world.
type State string

const (
	StateIdle     State = bridgecontract.WorldStateIdle
	StateStarting State = bridgecontract.WorldStateStarting
	StateRunning  State = bridgecontract.WorldStateRunning
	StateStopping State = bridgecontract.WorldStateStopping
	StateFailed   State = bridgecontract.WorldStateFailed // stays until closed so a failed open never falls through to another target
)

// World identifies the open world to Xbox friends.
type World struct {
	ID                  string `json:"id"`
	Name                string `json:"name"`
	GameMode            string `json:"game_mode"`
	Backend             string `json:"backend"`
	MultiplayerDisabled bool   `json:"multiplayer_disabled,omitempty"`
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
	mu         sync.Mutex
	status     Status
	generation uint64        // distinguishes reopening the same world after coalesced reports
	changed    chan struct{} // closed and replaced on every report
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
	if opened || (status.State == StateStarting && l.status.State == StateRunning) {
		l.generation++
	}
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

// Session binds advertised world metadata and routing to one opening of a local world.
type Session struct {
	World      World
	local      *Local
	generation uint64
}

// RunningSession captures a running world and its opening identity with the next-change channel.
func (l *Local) RunningSession() (session Session, running bool, changed <-chan struct{}) {
	l.mu.Lock()
	defer l.mu.Unlock()
	if l.status.State == StateRunning {
		return Session{World: *l.status.World, local: l, generation: l.generation}, true, l.changed
	}
	return Session{}, false, l.changed
}

// Current reports whether this exact opening and advertised metadata still run.
func (s Session) Current() bool {
	if s.local == nil {
		return false
	}
	s.local.mu.Lock()
	defer s.local.mu.Unlock()
	return s.currentLocked()
}

// currentLocked checks the captured opening while its Local mutex is held.
func (s Session) currentLocked() bool {
	return s.local.status.State == StateRunning && s.local.generation == s.generation &&
		*s.local.status.World == s.World
}

// ConnectionTarget returns only this running session's route, never a replacement world's target.
func (s Session) ConnectionTarget(ctx context.Context) (ConnectionTarget, bool, error) {
	if err := ctx.Err(); err != nil {
		return ConnectionTarget{}, false, err
	}
	if s.local == nil {
		return ConnectionTarget{}, false, ErrNotOpen
	}
	s.local.mu.Lock()
	defer s.local.mu.Unlock()
	if !s.currentLocked() {
		return ConnectionTarget{}, false, ErrNotOpen
	}
	return *s.local.status.Target, true, nil
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
