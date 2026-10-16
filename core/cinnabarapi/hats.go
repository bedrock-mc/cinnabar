package cinnabarapi

import (
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
)

type receivedHat struct {
	value    bridgecontract.CinnabarRemoteHat
	received time.Time
}

// collectHats prunes departed identities and expired leases while the session mutex is held.
func collectHats(s *session, state PlayerState, now time.Time) []bridgecontract.CinnabarRemoteHat {
	var result []bridgecontract.CinnabarRemoteHat
	for key, hat := range s.hats {
		age := uint64(max(time.Duration(0), now.Sub(hat.received)) / time.Millisecond)
		if age >= hat.value.ValidForMS || !watches(state, hat.value.XUID, hat.value.UUID) {
			delete(s.hats, key)
			continue
		}
		value := hat.value
		value.ValidForMS -= age
		result = append(result, value)
	}
	return result
}

// receiveHatAt admits only watched authenticated identities and bounded original-art identifiers.
func (c *Client) receiveHatAt(s *session, msg inbound, now time.Time) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if !ValidXUID(msg.XUID) || !ValidUUID(msg.UUID) || !watches(s.emoteState, msg.XUID, msg.UUID) {
		return
	}
	if msg.HatID == nil || *msg.HatID == "" || msg.ValidForMS == 0 {
		delete(s.hats, msg.XUID)
		return
	}
	if !sharedIDPattern.MatchString(*msg.HatID) || msg.ValidForMS > uint64(emoteTTL/time.Millisecond) {
		return
	}
	if s.hats == nil {
		s.hats = make(map[string]receivedHat)
	}
	if len(s.hats) >= bridgecontract.CinnabarAPIMaxLookupXUIDs {
		if _, exists := s.hats[msg.XUID]; !exists {
			return
		}
	}
	s.hats[msg.XUID] = receivedHat{value: bridgecontract.CinnabarRemoteHat{
		XUID: msg.XUID, UUID: msg.UUID, HatID: *msg.HatID, ValidForMS: msg.ValidForMS,
	}, received: now}
}
