// Generated from crates/server-experience/schema/extension.json; do not edit.
package cinnabarext

import (
	"errors"
)

type RateClass uint8

const (
	RateData RateClass = iota
	RateNativeEpoch
	RateWorldNotice
	RateWorldAck
	RateHandshake
)

// Cost counts encoded carrier bytes and messages, including escaping and fragment headers.
func (class RateClass) Cost(sizes []int) (RateCost, error) {
	var cost RateCost
	for _, size := range sizes {
		if size < 0 {
			return RateCost{}, errors.New("negative carrier size")
		}
		if size > MaxEnvelopeBytes {
			return RateCost{}, &BurstExceeded{}
		}
		cost.Messages++
		cost.Bytes += uint64(size)
		if cost.Messages > MaxMessagesPerSecond || cost.Bytes > MaxBurstBytes {
			return RateCost{}, &BurstExceeded{}
		}
	}
	switch class {
	case RateData:
		return cost, nil
	case RateNativeEpoch:
		return cost, nil
	case RateWorldNotice:
		return RateCost{}, nil
	case RateWorldAck:
		return RateCost{}, nil
	case RateHandshake:
		return RateCost{}, nil
	default:
		return RateCost{}, errors.New("unknown rate class")
	}
}

type RateCost struct{ Messages, Bytes uint64 }

// BurstExceeded means the encoded batch cannot fit even in a full bucket.
type BurstExceeded struct{}

// Error explains why retrying the unchanged batch cannot succeed.
func (*BurstExceeded) Error() string { return "encoded message too large for maximum rate burst" }

// RateClass classifies controls; acknowledgments are exempt only for an outstanding notice.
func (c Control) RateClass(direction Direction, matchingNotice bool) RateClass {
	switch {
	case c.Hello != nil:
		return RateHandshake
	case c.Accept != nil:
		return RateHandshake
	case c.Ready != nil:
		return RateHandshake
	case c.Epoch != nil:
		if direction == ToClient {
			return RateWorldNotice
		}
		return RateNativeEpoch
	case c.WorldAck != nil:
		if direction == ToServer && matchingNotice {
			return RateWorldAck
		}
		return RateData
	case c.Disabled:
		return RateHandshake
	default:
		return RateData
	}
}

// fitRate lowers the payload ceiling so UTF-8 fragmentation fits the message burst.
func (l Limits) fitRate() Limits {
	if l.MaxFragmentBytes < 4 {
		l.MaxMessageBytes = 0
		return l
	}
	l.MaxMessageBytes = uint32(min(uint64(l.MaxMessageBytes), uint64(l.MaxFragmentBytes-3)*MaxMessagesPerSecond))
	return l
}
