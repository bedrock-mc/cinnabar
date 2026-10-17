package cinnabarapi

import (
	"encoding/json"
	"regexp"
	"slices"
	"sort"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
)

var cosmeticIDPattern = regexp.MustCompile(`^[a-z0-9_.-]+:[a-z0-9_./-]+$`)
var cosmeticSlotPattern = regexp.MustCompile(`^([a-z0-9_.-]+:)?[a-z0-9_.-]+$`)

// validSlots bounds the local bridge selection before it reaches the API writer.
func validSlots(slots []bridgecontract.CinnabarCosmeticSlot) bool {
	if len(slots) > 16 {
		return false
	}
	seen := make(map[string]bool)
	for _, slot := range slots {
		if len(slot.Slot) > 48 || !cosmeticSlotPattern.MatchString(slot.Slot) || seen[slot.Slot] || len(slot.ItemID) > 128 || !cosmeticIDPattern.MatchString(slot.ItemID) {
			return false
		}
		seen[slot.Slot] = true
	}
	return true
}

// queueLoadout coalesces persistent selections separately from the leased emote channel.
func (c *Client) queueLoadout(s *session, slots []bridgecontract.CinnabarCosmeticSlot, now time.Time) {
	if !s.loadoutSent && len(slots) == 0 && s.loadoutRevision == 0 {
		s.loadoutSent = true
		return
	}
	if s.loadoutSent && slices.Equal(slots, s.loadoutSlots) {
		return
	}
	if !slices.Equal(slots, s.loadoutSlots) || !s.loadoutSent {
		if c.loadoutRevision >= maxBuildNumber {
			return
		}
		c.loadoutRevision = min(uint64(maxBuildNumber), max(c.loadoutRevision+1, uint64(max(int64(0), now.UnixMilli()))))
		s.loadoutRevision = c.loadoutRevision
		s.loadoutSlots = append([]bridgecontract.CinnabarCosmeticSlot(nil), slots...)
		s.loadoutSent = true
		select {
		case s.loadoutDirty <- struct{}{}:
		default:
		}
	}
}

// loadoutPayload snapshots the latest full replacement for the serialized writer.
func (c *Client) loadoutPayload(s *session) []byte {
	c.mu.Lock()
	defer c.mu.Unlock()
	slots := make(map[string]*string)
	for _, slot := range s.loadoutSlots {
		id := slot.ItemID
		slots[slot.Slot] = &id
	}
	payload, _ := json.Marshal(struct {
		Type     string             `json:"type"`
		Revision uint64             `json:"revision"`
		Slots    map[string]*string `json:"slots"`
	}{typeLoadout, s.loadoutRevision, slots})
	return payload
}

// collectLoadouts prunes departed identities without imposing a playback lease.
func collectLoadouts(s *session, state PlayerState) []bridgecontract.CinnabarRemoteLoadout {
	var result []bridgecontract.CinnabarRemoteLoadout
	for key, value := range s.loadouts {
		if !watches(state, value.XUID, value.UUID) {
			delete(s.loadouts, key)
			continue
		}
		result = append(result, value)
	}
	return result
}

// receiveLoadout retains only bounded authenticated snapshots from currently watched identities.
func (c *Client) receiveLoadout(s *session, msg inbound) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if !ValidXUID(msg.XUID) || !ValidUUID(msg.UUID) || !watches(s.emoteState, msg.XUID, msg.UUID) || msg.Revision > maxBuildNumber || len(msg.Slots) > 16 {
		return
	}
	slots := make([]bridgecontract.CinnabarCosmeticSlot, 0, len(msg.Slots))
	for slot, id := range msg.Slots {
		if id != nil {
			slots = append(slots, bridgecontract.CinnabarCosmeticSlot{Slot: slot, ItemID: *id})
		}
	}
	if !validSlots(slots) {
		return
	}
	sort.Slice(slots, func(i, j int) bool { return slots[i].Slot < slots[j].Slot })
	if s.loadouts == nil {
		s.loadouts = make(map[string]bridgecontract.CinnabarRemoteLoadout)
	}
	if previous, ok := s.loadouts[msg.XUID]; ok && previous.Revision > msg.Revision {
		return
	}
	if len(slots) == 0 {
		delete(s.loadouts, msg.XUID)
		return
	}
	if len(s.loadouts) >= bridgecontract.CinnabarAPIMaxLookupXUIDs {
		if _, ok := s.loadouts[msg.XUID]; !ok {
			return
		}
	}
	s.loadouts[msg.XUID] = bridgecontract.CinnabarRemoteLoadout{XUID: msg.XUID, UUID: msg.UUID, Revision: msg.Revision, Slots: slots}
}
