// Package testwait gives tests bounded waits instead of sleeps.
package testwait

import (
	"testing"
	"time"
)

// DefaultTimeout bounds background work in tests; a wait that reaches it fails the test.
const DefaultTimeout = 10 * time.Second

const maxBackoff = 20 * time.Millisecond

// WaitUntil polls cond with a short backoff until it holds, or reports false once timeout passes.
func WaitUntil(timeout time.Duration, cond func() bool) bool {
	deadline := time.Now().Add(timeout)
	backoff := 100 * time.Microsecond
	for {
		if cond() {
			return true
		}
		remaining := time.Until(deadline)
		if remaining <= 0 {
			return false
		}
		time.Sleep(min(backoff, remaining))
		backoff = min(backoff*2, maxBackoff)
	}
}

// Eventually fails t, naming what it waited for, if cond does not hold within timeout.
func Eventually(t testing.TB, timeout time.Duration, what string, cond func() bool) {
	t.Helper()
	if !WaitUntil(timeout, cond) {
		t.Fatalf("timed out after %v waiting for %s", timeout, what)
	}
}

// Idle pauses briefly between iterations of a bounded loop that also drives work.
func Idle() { time.Sleep(time.Millisecond) }
