package cinnabarapi

import (
	"encoding/json"
	"reflect"
	"regexp"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
)

const emoteTTL = 15 * time.Second
const emoteSpacing = 500 * time.Millisecond

var sharedIDPattern = regexp.MustCompile(`^[a-z0-9_.:-]{1,128}$`)

// PlayerState replaces nearby subscriptions, local playback and the equipped loadout.
type PlayerState = bridgecontract.CinnabarPlayerState

// RemoteEmote is a leased animation from an authenticated account.
type RemoteEmote = bridgecontract.CinnabarRemoteEmote

// PlayerUpdates contains leased playback and loadouts from authenticated nearby accounts.
type PlayerUpdates = bridgecontract.CinnabarPlayerUpdates

type emoteMessage struct {
	Type      string    `json:"type"`
	XUIDs     *[]string `json:"xuids,omitempty"`
	UUIDs     *[]string `json:"uuids,omitempty"`
	EmoteID   *string   `json:"emote_id"`
	ElapsedMS uint32    `json:"elapsed_ms"`
}

type receivedEmote struct {
	value    RemoteEmote
	received time.Time
}

// ValidPlayerState rejects unbounded subscriptions and malformed art identifiers.
func ValidPlayerState(state PlayerState) bool {
	return (state.EmoteID == "" || sharedIDPattern.MatchString(state.EmoteID)) &&
		validSlots(state.Slots) &&
		(len(state.XUIDs)+len(state.UUIDs) == 0 || ValidLookup(state.XUIDs, state.UUIDs))
}

// SyncPlayers coalesces local updates without waiting for the network and returns watched playback and loadouts.
func (c *Client) SyncPlayers(state PlayerState) PlayerUpdates {
	return c.syncPlayers(state, time.Now())
}

// syncPlayers advances one deterministic exchange at the caller's observation time.
func (c *Client) syncPlayers(state PlayerState, now time.Time) PlayerUpdates {
	c.mu.Lock()
	defer c.mu.Unlock()
	s := c.live
	if s == nil || !ValidPlayerState(state) {
		return PlayerUpdates{}
	}
	c.queueLoadout(s, state.Slots, now)
	changed := s.emoteState.EmoteID != state.EmoteID || s.emoteState.PlaybackID != state.PlaybackID ||
		!reflect.DeepEqual(s.emoteState.XUIDs, state.XUIDs) || !reflect.DeepEqual(s.emoteState.UUIDs, state.UUIDs)
	watchesPending := !reflect.DeepEqual(s.emoteSentXUIDs, state.XUIDs) || !reflect.DeepEqual(s.emoteSentUUIDs, state.UUIDs)
	watchDue := !s.emoteSent || now.Sub(s.emoteWatchSent) >= emoteTTL/5
	if now.Sub(s.emoteQueued) >= emoteSpacing && (changed || (watchesPending && watchDue) || now.Sub(s.emoteQueued) >= emoteTTL/3) {
		s.emoteState = state
		s.emoteQueued = now
		select {
		case s.emoteDirty <- struct{}{}:
		default:
		}
	}
	s.emoteState.XUIDs, s.emoteState.UUIDs = state.XUIDs, state.UUIDs
	var result []RemoteEmote
	for key, emote := range s.emotes {
		age := max(time.Duration(0), now.Sub(emote.received))
		if age >= time.Duration(emote.value.ValidForMS)*time.Millisecond || !watches(state, emote.value.XUID, emote.value.UUID) {
			delete(s.emotes, key)
			continue
		}
		value := emote.value
		value.ElapsedMS += uint64(age / time.Millisecond)
		value.ValidForMS -= uint64(age / time.Millisecond)
		result = append(result, value)
	}
	return PlayerUpdates{Emotes: result, Loadouts: collectLoadouts(s, state),
		OwnershipXUID: s.ownershipXUID, OwnedCosmetics: append([]string(nil), s.ownedCosmetics...)}
}

// watches resolves either identity namespace while keeping subscription memory bounded.
func watches(state PlayerState, xuid, uuid string) bool {
	for _, id := range state.XUIDs {
		if id == xuid {
			return true
		}
	}
	for _, id := range state.UUIDs {
		if id == uuid {
			return true
		}
	}
	return false
}

// receiveEmote admits authenticated identities only when this connection currently watches them.
func (c *Client) receiveEmote(s *session, msg inbound) {
	c.receiveEmoteAt(s, msg, time.Now())
}

// receiveEmoteAt retains one validated message at the caller's receipt time.
func (c *Client) receiveEmoteAt(s *session, msg inbound, now time.Time) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if !ValidXUID(msg.XUID) || !ValidUUID(msg.UUID) || !watches(s.emoteState, msg.XUID, msg.UUID) {
		return
	}
	if msg.EmoteID == nil || *msg.EmoteID == "" || msg.ValidForMS == 0 {
		delete(s.emotes, msg.XUID)
		return
	}
	if !sharedIDPattern.MatchString(*msg.EmoteID) || msg.ValidForMS > uint64(emoteTTL/time.Millisecond) {
		return
	}
	if len(s.emotes) >= bridgecontract.CinnabarAPIMaxLookupXUIDs {
		if _, exists := s.emotes[msg.XUID]; !exists {
			return
		}
	}
	s.emotes[msg.XUID] = receivedEmote{value: RemoteEmote{XUID: msg.XUID, UUID: msg.UUID, EmoteID: *msg.EmoteID,
		ElapsedMS: uint64(msg.ElapsedMS), ValidForMS: msg.ValidForMS}, received: now}
}

// emotePayload snapshots the newest coalesced state for the serialized WebSocket writer.
func (c *Client) emotePayload(s *session) []byte {
	c.mu.Lock()
	defer c.mu.Unlock()
	state := s.emoteState
	var id *string
	if state.EmoteID != "" {
		id = &state.EmoteID
	}
	message := emoteMessage{Type: typeEmoteState, EmoteID: id, ElapsedMS: state.ElapsedMS}
	changed := !reflect.DeepEqual(s.emoteSentXUIDs, state.XUIDs) || !reflect.DeepEqual(s.emoteSentUUIDs, state.UUIDs)
	if !s.emoteSent || (changed && s.emoteQueued.Sub(s.emoteWatchSent) >= emoteTTL/5) {
		xuids, uuids := append([]string{}, state.XUIDs...), append([]string{}, state.UUIDs...)
		message.XUIDs, message.UUIDs = &xuids, &uuids
		s.emoteSentXUIDs, s.emoteSentUUIDs = state.XUIDs, state.UUIDs
		s.emoteWatchSent = s.emoteQueued
		s.emoteSent = true
	}
	payload, _ := json.Marshal(message)
	return payload
}

// admittedOwnership rejects malformed server snapshots rather than accepting a partial grant list.
func admittedOwnership(ids []string) []string {
	if len(ids) > 256 {
		return nil
	}
	result := make([]string, 0, len(ids))
	seen := make(map[string]bool)
	for _, id := range ids {
		if len(id) > 128 || !cosmeticIDPattern.MatchString(id) {
			return nil
		}
		if !seen[id] {
			result = append(result, id)
			seen[id] = true
		}
	}
	return result
}
