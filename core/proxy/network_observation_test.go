package proxy

import (
	"testing"
	"time"
)

func TestRoundTripMillisPreservesUnknownAndRoundsFullRTT(t *testing.T) {
	for _, missing := range []time.Duration{-time.Second, 0} {
		if roundTripMillis(missing) != nil {
			t.Fatal("unmeasured latency must remain unknown")
		}
	}
	for _, test := range []struct {
		half time.Duration
		want uint32
	}{
		{time.Nanosecond, 1}, {1250 * time.Microsecond, 3}, {25 * time.Millisecond, 50}, {time.Duration(1<<63 - 1), ^uint32(0)},
	} {
		if got := roundTripMillis(test.half); got == nil || *got != test.want {
			t.Fatalf("half RTT %v: got %v, want %d", test.half, got, test.want)
		}
	}
}
