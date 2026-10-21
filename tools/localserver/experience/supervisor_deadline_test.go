package experience

import (
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
					_, err := h.roundTrip([]byte("callback"), resultDeadline)
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
