package xboxpresence

import (
	"context"
	"github.com/df-mc/go-xsapi/v2/presence"
	"log/slog"
	"sync"
	"time"
)

const (
	fallbackHeartbeat = 5 * time.Minute
	retryInterval     = 15 * time.Second
	requestTimeout    = 10 * time.Second
	// Leave time for other shutdown work before the parent escalates.
	cleanupTimeout = 500 * time.Millisecond
)

// Client owns one Xbox connection for the entire title presence lifetime.
type Client struct {
	Update func(context.Context, presence.TitleRequest) (*presence.UpdateResult, error)
	Close  func(context.Context) error
}

// Factory opens the signed-in account's Xbox connection on the worker.
type Factory func(context.Context) (*Client, error)

// Worker keeps Xbox requests off gameplay and coalesces superseded client states.
type Worker struct {
	ctx    context.Context
	cancel context.CancelFunc
	done   chan struct{}
	wake   chan struct{}
	mu     sync.Mutex
	state  State
	active context.CancelFunc
}

// New starts the presence lifetime; a nil factory leaves offline accounts untouched.
func New(ctx context.Context, factory Factory, logger *slog.Logger) *Worker {
	ctx, cancel := context.WithCancel(ctx)
	w := &Worker{ctx: ctx, cancel: cancel, done: make(chan struct{}), wake: make(chan struct{}, 1)}
	if logger == nil {
		logger = slog.New(slog.DiscardHandler)
	}
	go w.run(factory, logger)
	return w
}

// Set queues the newest state and cancels a request for an older state without waiting.
func (w *Worker) Set(state State) {
	w.mu.Lock()
	defer w.mu.Unlock()
	if w.state == state || w.ctx.Err() != nil {
		return
	}
	w.state = state
	if w.active != nil {
		w.active()
	}
	select {
	case w.wake <- struct{}{}:
	default:
	}
}

// Close ends updates and waits for bounded title cleanup; repeating it is safe.
func (w *Worker) Close() { w.cancel(); <-w.done }

// operation snapshots the newest state and binds its request to this presence lifetime.
func (w *Worker) operation() (State, context.Context, context.CancelFunc) {
	w.mu.Lock()
	defer w.mu.Unlock()
	ctx, cancel := context.WithTimeout(w.ctx, requestTimeout)
	w.active = cancel
	return w.state, ctx, cancel
}

// run serializes service requests and refreshes on the service's heartbeat deadline.
func (w *Worker) run(factory Factory, logger *slog.Logger) {
	defer close(w.done)
	if factory == nil {
		return
	}
	var client *Client
	defer func() {
		if client != nil {
			ctx, cancel := context.WithTimeout(context.Background(), cleanupTimeout)
			defer cancel()
			if client.Close(ctx) != nil {
				logger.Warn("Xbox presence cleanup failed")
			}
		}
	}()
	for w.ctx.Err() == nil {
		select {
		case <-w.wake:
		default:
		}
		state, ctx, cancel := w.operation()
		var err error
		if client == nil {
			client, err = factory(ctx)
		}
		delay := retryInterval
		if err == nil {
			var result *presence.UpdateResult
			result, err = client.Update(ctx, state.request())
			if err == nil {
				delay = heartbeatDelay(result)
			}
		}
		superseded := ctx.Err() == context.Canceled
		cancel()
		if err != nil && !superseded && w.ctx.Err() == nil {
			logger.Warn("Xbox presence update failed")
		}
		timer := time.NewTimer(delay)
		select {
		case <-w.ctx.Done():
			timer.Stop()
			return
		case <-w.wake:
			timer.Stop()
		case <-timer.C:
		}
	}
}

// heartbeatDelay uses the service deadline, falling back when none was returned.
func heartbeatDelay(result *presence.UpdateResult) time.Duration {
	if result != nil && result.HeartbeatAfter > 0 {
		return result.HeartbeatAfter
	}
	return fallbackHeartbeat
}
