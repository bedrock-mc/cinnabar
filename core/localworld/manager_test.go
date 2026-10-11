package localworld

import (
	"context"
	"errors"
	"sync"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/testwait"
)

type fakeInstance struct {
	mu      sync.Mutex
	paused  []bool
	stopped bool
	done    chan struct{}
}

func newFakeInstance() *fakeInstance { return &fakeInstance{done: make(chan struct{})} }

func (f *fakeInstance) Address() string { return "127.0.0.1:1" }
func (f *fakeInstance) SetPaused(p bool) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.paused = append(f.paused, p)
	return nil
}
func (f *fakeInstance) Stop(context.Context) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	if !f.stopped {
		f.stopped = true
		close(f.done)
	}
	return nil
}
func (f *fakeInstance) Done() <-chan struct{} { return f.done }
func (f *fakeInstance) wasStopped() bool {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.stopped
}

type fakeRunner struct {
	gate chan struct{} // when set, Start blocks until it is closed or ctx ends
	err  error
	mu   sync.Mutex
	made []*fakeInstance
}

func (r *fakeRunner) Start(ctx context.Context, _ StartSpec) (Instance, error) {
	if r.gate != nil {
		select {
		case <-r.gate:
		case <-ctx.Done():
			return nil, ctx.Err()
		}
	}
	if r.err != nil {
		return nil, r.err
	}
	inst := newFakeInstance()
	r.mu.Lock()
	r.made = append(r.made, inst)
	r.mu.Unlock()
	return inst, nil
}

func (r *fakeRunner) last() *fakeInstance {
	r.mu.Lock()
	defer r.mu.Unlock()
	return r.made[len(r.made)-1]
}

func waitState(t *testing.T, m *Manager, want State) Status {
	t.Helper()
	var status Status
	if !testwait.WaitUntil(2*time.Second, func() bool {
		status = m.Status()
		return status.State == want
	}) {
		t.Fatalf("state = %v, want %v", status, want)
	}
	return status
}

func newTestManager(t *testing.T, runner Runner) (*Manager, World) {
	t.Helper()
	m := NewManager(newTestStore(t), runner, nil)
	world, err := m.Create(Spec{Name: "w", Generator: GeneratorFlat})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(m.Shutdown)
	return m, world
}

func TestOpenRunsThenCloseStopsServer(t *testing.T) {
	runner := &fakeRunner{}
	m, world := newTestManager(t, runner)
	if err := m.Open(world.ID); err != nil {
		t.Fatal(err)
	}
	if status := waitState(t, m, StateRunning); status.WorldID != world.ID {
		t.Fatalf("status = %+v", status)
	}
	addr, ok, err := m.Target(context.Background())
	if addr != "127.0.0.1:1" || !ok || err != nil {
		t.Fatalf("target = %q %v %v", addr, ok, err)
	}
	if err := m.Open(world.ID); err != nil {
		t.Fatalf("reopen of running world must be a no-op: %v", err)
	}
	inst := runner.last()
	_ = m.Close()
	waitState(t, m, StateIdle)
	if !inst.wasStopped() {
		t.Fatal("server not stopped")
	}
	if _, ok, _ := m.Target(context.Background()); ok {
		t.Fatal("target still reported after close")
	}
}

func TestOpenAnotherWorldWhileRunningIsBusy(t *testing.T) {
	m, world := newTestManager(t, &fakeRunner{})
	other, _ := m.Create(Spec{Name: "other", Generator: GeneratorFlat})
	_ = m.Open(world.ID)
	waitState(t, m, StateRunning)
	if err := m.Open(other.ID); !errors.Is(err, ErrBusy) {
		t.Fatalf("got %v", err)
	}
	if err := m.Delete(world.ID); !errors.Is(err, ErrInUse) {
		t.Fatalf("delete open world: %v", err)
	}
	if err := m.Delete(other.ID); err != nil {
		t.Fatalf("delete other world: %v", err)
	}
}

func TestOpenUnknownWorldFails(t *testing.T) {
	m, _ := newTestManager(t, &fakeRunner{})
	if err := m.Open("0123456789abcdef"); !errors.Is(err, ErrNotFound) {
		t.Fatalf("got %v", err)
	}
	if m.Status().State != StateIdle {
		t.Fatalf("state = %v", m.Status())
	}
}

func TestStartFailureIsFailedUntilClosedAndHidesDetail(t *testing.T) {
	m, world := newTestManager(t, &fakeRunner{err: errors.New("exec /secret/path: boom")})
	_ = m.Open(world.ID)
	status := waitState(t, m, StateFailed)
	if status.Error == "" || status.Error == "exec /secret/path: boom" {
		t.Fatalf("error = %q", status.Error)
	}
	if _, ok, err := m.Target(context.Background()); ok || err == nil {
		t.Fatalf("failed world must error, not fall through: %v %v", ok, err)
	}
	_ = m.Close()
	waitState(t, m, StateIdle)
}

func TestCloseWhileStartingCancelsStart(t *testing.T) {
	runner := &fakeRunner{gate: make(chan struct{})}
	m, world := newTestManager(t, runner)
	_ = m.Open(world.ID)
	_ = m.Close()
	waitState(t, m, StateIdle)
}

func TestTargetWaitsForStartAndHonoursContext(t *testing.T) {
	runner := &fakeRunner{gate: make(chan struct{})}
	m, world := newTestManager(t, runner)
	_ = m.Open(world.ID)
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Millisecond)
	defer cancel()
	if _, _, err := m.Target(ctx); !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("got %v", err)
	}
	got := make(chan string, 1)
	go func() {
		addr, _, _ := m.Target(context.Background())
		got <- addr
	}()
	close(runner.gate)
	select {
	case addr := <-got:
		if addr != "127.0.0.1:1" {
			t.Fatalf("addr = %q", addr)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("Target did not return after start")
	}
}

func TestPauseAppliesWhileRunningAndQueuesWhileStarting(t *testing.T) {
	runner := &fakeRunner{gate: make(chan struct{})}
	m, world := newTestManager(t, runner)
	if err := m.SetPaused(true); !errors.Is(err, ErrNotOpen) {
		t.Fatalf("idle pause: %v", err)
	}
	_ = m.Open(world.ID)
	if err := m.SetPaused(true); err != nil {
		t.Fatal(err)
	}
	close(runner.gate)
	waitState(t, m, StateRunning)
	inst := runner.last()
	testwait.WaitUntil(2*time.Second, func() bool {
		inst.mu.Lock()
		defer inst.mu.Unlock()
		return len(inst.paused) > 0
	})
	if err := m.SetPaused(false); err != nil {
		t.Fatal(err)
	}
	inst.mu.Lock()
	defer inst.mu.Unlock()
	if len(inst.paused) != 2 || !inst.paused[0] || inst.paused[1] || m.Status().Paused {
		t.Fatalf("pause calls = %v", inst.paused)
	}
}

func TestUnexpectedExitFails(t *testing.T) {
	runner := &fakeRunner{}
	m, world := newTestManager(t, runner)
	_ = m.Open(world.ID)
	waitState(t, m, StateRunning)
	_ = runner.last().Stop(context.Background())
	waitState(t, m, StateFailed)
}
