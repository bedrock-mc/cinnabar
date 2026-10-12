package proxy

import "time"

// NetworkObservation identifies the active upstream and its current transport RTT.
// A nil sampler withdraws this attempt; unsupported or unmeasured RTT stays nil.
type NetworkObservation struct {
	AttemptID       uint64
	LocalRuntimeID  uint64
	RoundTripMillis func() *uint32
}

// latencySampler converts the upstream's half-RTT estimate without measuring the local bridge.
func latencySampler(upstream upstreamSession) func() *uint32 {
	connection, ok := upstream.(interface{ Latency() time.Duration })
	if !ok {
		return func() *uint32 { return nil }
	}
	return func() (sample *uint32) {
		// Some transport adapters expose Conn.Latency but have no underlying estimator.
		defer func() {
			if recover() != nil {
				sample = nil
			}
		}()
		return roundTripMillis(connection.Latency())
	}
}

// roundTripMillis rounds a positive half-RTT estimate up to full milliseconds.
func roundTripMillis(half time.Duration) *uint32 {
	if half <= 0 {
		return nil
	}
	const maxMillis = uint64(^uint32(0))
	// Quotient/remainder arithmetic avoids overflowing the duration when doubling.
	whole := uint64(half / time.Millisecond)
	fraction := half % time.Millisecond
	millis := min(maxMillis, whole*2+uint64((fraction*2+time.Millisecond-1)/time.Millisecond))
	value := uint32(millis)
	return &value
}
