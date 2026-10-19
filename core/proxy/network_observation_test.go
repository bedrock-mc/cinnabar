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

// Both native observations and relay traces share one optional transport estimator.
func TestLatencyConsumersPreserveMissingOrUnavailableEstimators(t *testing.T) {
	for _, latency := range []func() time.Duration{
		nil,
		func() time.Duration { panic("adapter has no estimator") },
	} {
		session := &InterceptedSession{latency: latency}
		if got := session.Latency(); got != 0 {
			t.Fatalf("native latency = %v; want unknown", got)
		}
		if got := sampleRoundTrip(latency); got != 0 {
			t.Fatalf("trace RTT = %v; want unknown", got)
		}
	}
	if got := latencySampler(latencyUpstream{})(); got == nil || *got != 14 {
		t.Fatalf("network RTT = %v; want 14ms", got)
	}
}
