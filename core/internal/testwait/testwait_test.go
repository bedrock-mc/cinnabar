package testwait

import (
	"sync/atomic"
	"testing"
	"time"
)

func TestWaitUntilSeesWorkFromAnotherGoroutine(t *testing.T) {
	var done atomic.Bool
	go done.Store(true)
	if !WaitUntil(DefaultTimeout, done.Load) {
		t.Fatal("never saw the goroutine's write")
	}
}

func TestWaitUntilGivesUpAtItsTimeout(t *testing.T) {
	start := time.Now()
	if WaitUntil(30*time.Millisecond, func() bool { return false }) {
		t.Fatal("a false condition held")
	}
	if elapsed := time.Since(start); elapsed < 30*time.Millisecond {
		t.Fatalf("gave up after %v", elapsed)
	}
}
