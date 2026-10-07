package authcache

import (
	"context"
	"errors"
	"net/http"
	"time"

	"github.com/sandertv/gophertunnel/minecraft/auth"
)

const authRequestTimeout = 15 * time.Second // one auth HTTP request, so an unreachable host fails instead of hanging

var derivationTimeout = 30 * time.Second // one shared derivation, however many requests it chains

// authHTTPClient sends every Xbox, PlayFab and service-auth request the account makes.
var authHTTPClient = &http.Client{Timeout: authRequestTimeout}

var errDerivationTimeout = errors.New("authentication: credential request timed out")

// flight is one in-progress derivation; value and err are set before done closes.
type flight struct {
	done  chan struct{}
	value any
	err   error
}

// awaitFlight returns the result of the derivation running under key, starting it when none is.
// The run is detached from every caller: it ends with the account or after derivationTimeout, and a
// caller whose ctx ends returns at once while the run continues for the others.
func awaitFlight[T any](s *Account, ctx context.Context, key string, run func(context.Context) (T, error)) (T, error) {
	var zero T
	if err := ctx.Err(); err != nil {
		return zero, err
	}
	s.flightMu.Lock()
	f := s.flights[key]
	if f == nil {
		if err := s.begin(); err != nil {
			s.flightMu.Unlock()
			return zero, err
		}
		f = &flight{done: make(chan struct{})}
		if s.flights == nil {
			s.flights = make(map[string]*flight)
		}
		s.flights[key] = f
		go s.fly(key, f, func(ctx context.Context) (any, error) { return run(ctx) })
	}
	s.flightMu.Unlock()
	select {
	case <-f.done:
		value, _ := f.value.(T)
		return value, f.err
	case <-ctx.Done():
		return zero, ctx.Err()
	}
}

// fly completes f when run returns or its deadline passes, whichever is first; a run still unwinding
// past its deadline keeps the account's Close waiting, never the flight's callers.
func (s *Account) fly(key string, f *flight, run func(context.Context) (any, error)) {
	ctx, cancel := context.WithTimeout(s.ctx, derivationTimeout)
	type result struct {
		value any
		err   error
	}
	done := make(chan result, 1)
	go func() {
		defer s.end()
		value, err := run(auth.WithContextClient(ctx, s.http))
		done <- result{value, err}
	}()
	var r result
	select {
	case r = <-done:
	case <-ctx.Done():
		r.err = ctx.Err()
	}
	if r.err != nil && errors.Is(ctx.Err(), context.DeadlineExceeded) {
		r.err = errDerivationTimeout // not the waiting caller's own deadline
	}
	cancel() // only once the result is taken, so a finished run is never mistaken for a cancelled one
	s.flightMu.Lock()
	delete(s.flights, key)
	s.flightMu.Unlock()
	f.value, f.err = r.value, r.err
	close(f.done)
}

// begin registers an operation Close waits for; it refuses new work once Close has begun.
func (s *Account) begin() error {
	s.activeMu.Lock()
	defer s.activeMu.Unlock()
	if s.closing || s.closed.Load() {
		return ErrAccountClosed
	}
	s.active++
	return nil
}

func (s *Account) end() {
	s.activeMu.Lock()
	defer s.activeMu.Unlock()
	if s.active--; s.active == 0 && s.idle != nil {
		close(s.idle)
		s.idle = nil
	}
}

// drain refuses new operations and waits for those already running, which the cancelled account
// context ends promptly.
func (s *Account) drain() {
	s.activeMu.Lock()
	s.closing = true
	if s.active == 0 {
		s.activeMu.Unlock()
		return
	}
	if s.idle == nil {
		s.idle = make(chan struct{})
	}
	idle := s.idle
	s.activeMu.Unlock()
	<-idle
}
