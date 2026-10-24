package experience

import (
	"context"
	"errors"
	"fmt"
	"io"
	"testing"
	"time"
)

// TestHelperOutcomeAccounting charges only guest failures, including when cancellation races them.
func TestHelperOutcomeAccounting(t *testing.T) {
	failures := []string{"committed", "rejected", "protocol", "transport"}
	for _, kind := range failKinds {
		failures = append(failures, string(kind))
	}
	for _, host := range []string{"active", "cancelled", "deadline"} {
		for _, failure := range failures {
			t.Run(host+"/"+failure, func(t *testing.T) {
				log, _ := testLog(t)
				s := &Supervisor{opts: startOptions{now: newFakeClock().Now}}
				s.log.Store(log)
				ctx := context.Background()
				switch host {
				case "cancelled":
					cancelled, cancel := context.WithCancel(ctx)
					cancel()
					ctx = cancelled
				case "deadline":
					expired, cancel := context.WithDeadline(ctx, time.Time{})
					defer cancel()
					ctx = expired
				}
				var out Outcome
				var err error
				guest := true
				switch failure {
				case "committed":
					out.Committed = &Committed{}
					guest = false
				case "rejected":
					out.Rejected = &Rejected{}
					guest = false
				case "protocol":
					err = fmt.Errorf("%w: invalid result", errGuestFault)
				case "transport":
					err = io.ErrClosedPipe
					guest = false
				default:
					out.Failed = &Failed{Kind: FailKind(failure)}
				}
				s.account(ctx, out, err)
				want := 0
				if host == "active" && guest {
					want = 1
				}
				if len(s.strikes) != want || s.Quarantined() {
					t.Fatalf("strikes = %d, want %d; quarantined = %v", len(s.strikes), want, s.Quarantined())
				}
			})
		}
	}
}

// TestCancelledRestartPreservesBudgets keeps an interrupted load recoverable with fresh context.
func TestCancelledRestartPreservesBudgets(t *testing.T) {
	for _, point := range []string{"before_load", "awaiting_load"} {
		t.Run(point, func(t *testing.T) {
			log, _ := testLog(t)
			s, _ := startFake(t, "ok", log, startOptions{})
			for range strikeLimit - 1 {
				s.account(t.Context(), Outcome{Failed: &Failed{Kind: FailTrap}}, nil)
			}
			for range maxRestarts {
				s.restarts = append(s.restarts, s.opts.now())
			}
			old := s.proc
			old.kill()
			s.proc = nil
			ctx, cancel := context.WithCancel(t.Context())
			defer cancel()
			var interrupted *helper
			done := make(chan struct{})
			s.opts.spawned = func(h *helper) {
				interrupted = h
				if point == "before_load" {
					cancel()
					close(done)
					return
				}
				response := &heldResponse{ReadCloser: h.stdout, started: make(chan struct{}), released: make(chan struct{})}
				h.stdout = response
				go func() {
					defer close(done)
					<-response.started
					cancel()
				}()
			}
			if err := s.restart(ctx); !errors.Is(err, context.Canceled) {
				t.Fatalf("restart = %v, want host cancellation", err)
			}
			<-done
			assertReaped(t, interrupted)
			if s.Quarantined() || len(s.strikes) != strikeLimit-1 || len(s.restarts) != maxRestarts {
				t.Fatalf("cancelled load changed budgets: strikes=%d, restarts=%d, quarantined=%v", len(s.strikes), len(s.restarts), s.Quarantined())
			}
			s.opts.spawned = nil
			if out, err := s.Call(probeInteract(probeCount)); err != nil || out.Committed == nil {
				t.Fatalf("fresh call = %+v, %v; want committed", out, err)
			}
		})
	}
}
