package experience

import (
	"context"
	"errors"
	"fmt"
	"io"
	"strings"
	"testing"
	"testing/synctest"
	"time"
)

// Both a blocked write and a missing response consume exactly the result deadline.
func TestRoundTripHonorsDeadline(t *testing.T) {
	for _, drain := range []bool{false, true} {
		t.Run(fmt.Sprintf("drain_request=%t", drain), func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				requests, stdin := io.Pipe()
				stdout, responses := io.Pipe()
				defer requests.Close()
				defer stdin.Close()
				defer stdout.Close()
				defer responses.Close()
				if drain {
					go io.Copy(io.Discard, requests)
				}
				h := helper{stdin: stdin, stdout: stdout}
				done := make(chan error, 1)
				started := time.Now()
				go func() {
					_, err := h.roundTrip(context.Background(), []byte("callback"), resultDeadline)
					done <- err
				}()
				synctest.Wait()
				<-time.After(resultDeadline - time.Nanosecond)
				select {
				case err := <-done:
					t.Fatalf("roundTrip returned before the deadline: %v", err)
				default:
				}
				<-time.After(time.Nanosecond)
				if err := <-done; err == nil || !strings.Contains(err.Error(), "no answer within") {
					t.Fatalf("roundTrip: %v, want deadline fault", err)
				}
				if elapsed := time.Since(started); elapsed != resultDeadline {
					t.Fatalf("roundTrip returned after %v, want %v", elapsed, resultDeadline)
				}
			})
		})
	}
}

// TestRoundTripCancellation releases blocked callback and load I/O as soon as their host cancels.
func TestRoundTripCancellation(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		requests, stdin := io.Pipe()
		stdout, responses := io.Pipe()
		defer requests.Close()
		defer stdin.Close()
		defer stdout.Close()
		defer responses.Close()
		h := helper{stdin: stdin, stdout: stdout}
		ctx, cancel := context.WithCancel(context.Background())
		defer cancel()
		done := make(chan error, 1)
		go func() { _, err := h.roundTrip(ctx, []byte("callback"), resultDeadline); done <- err }()
		synctest.Wait()
		cancel()
		synctest.Wait()
		select {
		case err := <-done:
			if !errors.Is(err, context.Canceled) {
				t.Fatalf("roundTrip cancellation = %v", err)
			}
		default:
			t.Fatal("callback I/O ignored host cancellation")
		}
	})
}
