package localworld

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"sync"
	"time"
)

// State is the lifecycle of the single open world.
type State string

const (
	StateIdle     State = "idle"
	StateStarting State = "starting"
	StateRunning  State = "running"
	StateStopping State = "stopping"
	StateFailed   State = "failed" // stays until Close so a failed open never falls through to another target
)

const (
	defaultStopTimeout = 30 * time.Second // BDS saves the world before exiting
	defaultRuntimeWait = 25 * time.Second // outlasts the Docker probe's own timeout
)

// Status is the secret-safe view of the open world; Error never carries paths.
type Status struct {
	State   State  `json:"state"`
	WorldID string `json:"world_id,omitempty"`
	Backend string `json:"backend,omitempty"` // of the open world
	Paused  bool   `json:"paused"`
	// PauseSupported is false when the open world's server cannot pause (BDS).
	PauseSupported bool         `json:"pause_supported"`
	Error          string       `json:"error,omitempty"`
	Setup          *SetupStatus `json:"setup,omitempty"` // dedicated-server acquisition, when configured
	// BackendUnavailableReason mirrors Setup's docker_missing / docker_not_running.
	BackendUnavailableReason string `json:"backend_unavailable_reason,omitempty"`
}

// StartSpec identifies the world a Runner must host.
type StartSpec struct {
	World   World
	Dir     string
	Options OpenOptions
}

// Instance is one running local server.
type Instance interface {
	// Address is the loopback game address.
	Address() string
	SetPaused(paused bool) error
	// Stop shuts the server down gracefully, killing it when ctx ends first.
	Stop(ctx context.Context) error
	// Done is closed once the server has exited.
	Done() <-chan struct{}
}

// Runner launches local servers; Start returns only once the server accepts connections.
type Runner interface {
	Start(ctx context.Context, spec StartSpec) (Instance, error)
}

// Manager owns the store and at most one running local world.
type Manager struct {
	setup       Setup            // nil when no dedicated-server backend is configured
	autoBackend bool             // Prefs re-probes may change the default backend
	unavailable map[string]error // backends that cannot host worlds here, with the operator-facing reason
	store       *Store
	runner      Runner
	log         *slog.Logger
	stopTimeout time.Duration
	runtimeWait time.Duration // how long Create waits for a runtime detection in flight
	bg          sync.WaitGroup

	mu          sync.Mutex
	state       State
	world       World
	inst        Instance
	paused      bool
	failure     string
	cancelStart context.CancelFunc
	changed     chan struct{} // closed and replaced on every state change
}

func NewManager(store *Store, runner Runner, log *slog.Logger) *Manager {
	if log == nil {
		log = slog.Default()
	}
	return &Manager{store: store, runner: runner, log: log, stopTimeout: defaultStopTimeout, runtimeWait: defaultRuntimeWait, state: StateIdle, changed: make(chan struct{})}
}

func (m *Manager) setState(state State) {
	m.state = state
	close(m.changed)
	m.changed = make(chan struct{})
}

func (m *Manager) idleLocked() {
	m.world, m.inst, m.paused, m.failure, m.cancelStart = World{}, nil, false, "", nil
	m.setState(StateIdle)
}

// SetSetup attaches the dedicated-server installer whose status is reported and whose EULA gates BDS worlds.
// Each detection result that becomes current re-points the default backend.
func (m *Manager) SetSetup(setup Setup) {
	m.setup = setup
	setup.OnDetected(m.runtimeDetected)
}

// SetAutoBackend lets a Docker re-probe change the default backend (false when the operator forced one).
func (m *Manager) SetAutoBackend(auto bool) { m.autoBackend = auto }

// SetUnavailable marks backend as unable to host worlds; Create refuses it, logging reason.
func (m *Manager) SetUnavailable(backend string, reason error) {
	if m.unavailable == nil {
		m.unavailable = map[string]error{}
	}
	m.unavailable[backend] = reason
}

// AcceptEULA records EULA acceptance so BDS worlds may download and start the server.
func (m *Manager) AcceptEULA() error {
	if m.setup == nil {
		return ErrBackendUnavailable
	}
	return m.setup.AcceptEULA()
}

// Prefs applies update (re-probing Docker first when asked) and returns the saved preferences.
// A re-probe only changes the default backend of worlds created afterwards; saved worlds keep theirs.
func (m *Manager) Prefs(ctx context.Context, update PrefsUpdate) (Prefs, error) {
	if update.Redetect && m.setup != nil {
		m.setup.Redetect(ctx)
	}
	if update.DockerPromptDismissed == nil {
		return m.store.Prefs(), nil
	}
	return m.store.UpdatePrefs(update)
}

// runtimeDetected points the default backend at a fresh probe's result, unless the operator forced one.
func (m *Manager) runtimeDetected(info RuntimeInfo) {
	m.log.Info("local world runtime detected", "bds_runtime", info.Kind, "reason", info.Reason)
	if m.autoBackend {
		backend := DefaultBackend(info)
		m.store.SetDefaultBackend(backend)
		if reason := m.unavailable[backend]; reason != nil {
			m.log.Error("default local world backend is unavailable", "backend", backend, "error", reason)
		}
	}
}

// Runners routes a world to the runner of its backend.
type Runners map[string]Runner

func (r Runners) Start(ctx context.Context, spec StartSpec) (Instance, error) {
	runner, ok := r[spec.World.Backend]
	if !ok || runner == nil {
		return nil, ErrBackendUnavailable
	}
	return runner.Start(ctx, spec)
}

func (m *Manager) List() ([]World, error) { return m.store.List() }

// Create saves a new world; a BDS world (every normal world) is refused where BDS cannot run.
// Unless Dragonfly was asked for, it first waits out a runtime detection in flight, so no world is saved
// against a guessed backend.
func (m *Manager) Create(spec Spec) (World, error) {
	normalized, err := spec.normalize()
	if err != nil {
		return World{}, err
	}
	if m.setup != nil && normalized.Backend != BackendDragonfly {
		ctx, cancel := context.WithTimeout(context.Background(), m.runtimeWait)
		err := m.setup.AwaitRuntime(ctx)
		cancel()
		if err != nil {
			return World{}, ErrRuntimePending
		}
	}
	if normalized.Backend == "" {
		normalized.Backend = m.store.DefaultBackend()
	}
	if reason := m.unavailable[normalized.Backend]; reason != nil {
		m.log.Error("cannot create local world", "backend", normalized.Backend, "error", reason)
		return World{}, fmt.Errorf("%w: %s", ErrBackendUnavailable, normalized.Backend)
	}
	if normalized.Backend == BackendBDS && !m.bdsRunnable() {
		if normalized.Generator == GeneratorNormal {
			return World{}, ErrVanillaNeedsBDS
		}
		return World{}, ErrBackendUnavailable
	}
	return m.store.Create(normalized)
}

func (m *Manager) bdsRunnable() bool {
	return m.setup != nil && m.setup.Status().State != SetupUnsupported
}

func (m *Manager) Update(id string, update Update) (World, error) { return m.store.Update(id, update) }

// Delete removes a world that is not starting, running or stopping.
func (m *Manager) Delete(id string) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	if m.state != StateIdle && m.state != StateFailed && m.world.ID == id {
		return ErrInUse
	}
	return m.store.Delete(id)
}

// Status reports the open world's lifecycle.
func (m *Manager) Status() Status {
	m.mu.Lock()
	status := Status{State: m.state, WorldID: m.world.ID, Backend: m.world.Backend, Paused: m.paused, Error: m.failure}
	status.PauseSupported = m.inst == nil || canPause(m.inst)
	m.mu.Unlock()
	if m.setup != nil {
		setup := m.setup.Status()
		status.Setup = &setup
		status.BackendUnavailableReason = setup.UnavailableReason
	}
	return status
}

func failureText(err error) string {
	for _, known := range []error{ErrEULARequired, ErrDockerNotRunning, ErrBackendUnavailable, ErrVanillaNeedsBDS, ErrImageNotPinned} {
		if errors.Is(err, known) {
			return known.Error()
		}
	}
	return "local world server failed to start"
}

func canPause(inst Instance) bool {
	c, ok := inst.(interface{ CanPause() bool })
	return !ok || c.CanPause()
}

// Open begins starting a world and returns immediately; poll Status for readiness.
// Reopening the world that is already starting or running is a no-op.
func (m *Manager) Open(id string, opts ...OpenOptions) error {
	var options OpenOptions
	if len(opts) > 0 {
		options = opts[0]
	}
	m.mu.Lock()
	defer m.mu.Unlock()
	switch m.state {
	case StateStarting, StateRunning:
		if m.world.ID == id {
			return nil
		}
		return ErrBusy
	case StateStopping:
		return ErrBusy
	}
	world, err := m.store.Get(id)
	if err != nil {
		return err
	}
	dir, err := m.store.Dir(id)
	if err != nil {
		return err
	}
	if world.Backend == BackendDragonfly && world.Generator == GeneratorNormal {
		return ErrVanillaNeedsBDS // saved before normal worlds moved to BDS; never regenerate as an approximation
	}
	if world.Backend == BackendBDS {
		if m.setup == nil {
			return ErrBackendUnavailable
		}
		switch setup := m.setup.Status(); {
		case setup.State == SetupUnsupported:
			return ErrBackendUnavailable
		case !setup.EULAAccepted:
			return ErrEULARequired
		}
	}
	ctx, cancel := context.WithCancel(context.Background())
	m.world, m.failure, m.paused, m.cancelStart = world, "", false, cancel
	m.setState(StateStarting)
	m.bg.Add(1)
	go m.start(ctx, StartSpec{World: world, Dir: dir, Options: options})
	return nil
}

func (m *Manager) start(ctx context.Context, spec StartSpec) {
	defer m.bg.Done()
	inst, err := m.runner.Start(ctx, spec)
	m.mu.Lock()
	if ctx.Err() != nil {
		m.mu.Unlock()
		if inst != nil {
			m.stopInstance(inst)
		}
		m.mu.Lock()
		m.idleLocked()
		m.mu.Unlock()
		return
	}
	if err != nil {
		m.log.Error("local world server failed to start", "world", spec.World.ID, "error", err)
		m.failure = failureText(err)
		m.setState(StateFailed)
		m.mu.Unlock()
		return
	}
	m.inst = inst
	paused := m.paused && canPause(inst)
	m.paused = paused
	m.setState(StateRunning)
	m.mu.Unlock()
	if err := m.store.Touch(spec.World.ID); err != nil {
		m.log.Warn("record last played failed", "world", spec.World.ID, "error", err)
	}
	if paused {
		_ = inst.SetPaused(true)
	}
	m.bg.Add(1)
	go m.watch(inst)
}

func (m *Manager) watch(inst Instance) {
	defer m.bg.Done()
	<-inst.Done()
	m.mu.Lock()
	defer m.mu.Unlock()
	if m.inst == inst && m.state == StateRunning {
		m.log.Error("local world server exited unexpectedly", "world", m.world.ID)
		m.inst = nil
		m.failure = "local world server exited unexpectedly"
		m.setState(StateFailed)
	}
}

func (m *Manager) stopInstance(inst Instance) {
	ctx, cancel := context.WithTimeout(context.Background(), m.stopTimeout)
	defer cancel()
	if err := inst.Stop(ctx); err != nil {
		m.log.Warn("stop local world server", "error", err)
	}
}

// Close stops the open world (saving it) or clears a failure; it returns without waiting for shutdown.
func (m *Manager) Close() error {
	m.mu.Lock()
	defer m.mu.Unlock()
	switch m.state {
	case StateFailed:
		m.idleLocked()
	case StateStarting:
		m.cancelStart()
		m.setState(StateStopping)
	case StateRunning:
		inst := m.inst
		m.inst = nil
		m.setState(StateStopping)
		m.bg.Add(1)
		go func() {
			defer m.bg.Done()
			m.stopInstance(inst)
			m.mu.Lock()
			m.idleLocked()
			m.mu.Unlock()
		}()
	}
	return nil
}

// Shutdown closes the open world and waits until its server has exited.
func (m *Manager) Shutdown() {
	_ = m.Close()
	m.bg.Wait()
}

// SetPaused freezes or resumes the running world; a request while starting applies once it is running.
func (m *Manager) SetPaused(paused bool) error {
	m.mu.Lock()
	switch m.state {
	case StateStarting:
		m.paused = paused
		m.mu.Unlock()
		return nil
	case StateRunning:
		inst := m.inst
		if !canPause(inst) {
			m.mu.Unlock()
			return nil
		}
		m.paused = paused
		m.mu.Unlock()
		return inst.SetPaused(paused)
	}
	m.mu.Unlock()
	return ErrNotOpen
}

// Target reports the local server address for the proxy, waiting out a start. ok is false when no world is open.
func (m *Manager) Target(ctx context.Context) (address string, ok bool, err error) {
	target, ok, err := m.ConnectionTarget(ctx)
	return target.Address, ok, err
}

// ConnectionTarget reports the local address and transport atomically, waiting out a start.
func (m *Manager) ConnectionTarget(ctx context.Context) (ConnectionTarget, bool, error) {
	for {
		m.mu.Lock()
		state, inst, changed, failure, world := m.state, m.inst, m.changed, m.failure, m.world
		m.mu.Unlock()
		switch state {
		case StateIdle:
			return ConnectionTarget{}, false, nil
		case StateRunning:
			transport := TransportRakNet
			switch world.Backend {
			case BackendBDS:
				transport = TransportNetherNetHTTP
			case BackendDragonfly:
			default:
				return ConnectionTarget{}, false, fmt.Errorf("local world uses unsupported backend %q", world.Backend)
			}
			target := ConnectionTarget{Address: inst.Address(), Transport: transport}
			if world.Backend == BackendBDS {
				if lan, ok := inst.(interface{ LANAddress() string }); ok && lan.LANAddress() != "" {
					target.Transport = TransportNetherNetLAN
					target.LANAddress = lan.LANAddress()
					target.LevelName = world.ID
				}
			}
			return target, true, nil
		case StateFailed:
			return ConnectionTarget{}, false, fmt.Errorf("local world unavailable: %s", failure)
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
