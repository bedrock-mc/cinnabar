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

var emoteIDPattern = regexp.MustCompile(`^[a-z0-9_.:-]{1,128}$`)

// EmoteState replaces nearby subscriptions and local playback.
type EmoteState = bridgecontract.CinnabarEmoteState

// RemoteEmote is a leased animation from an authenticated account.
type RemoteEmote = bridgecontract.CinnabarRemoteEmote

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

// Valid rejects unbounded subscriptions and clips before they reach the shared connection.
func ValidEmoteState(state EmoteState) bool {
	return (state.EmoteID == "" || emoteIDPattern.MatchString(state.EmoteID)) &&
		(len(state.XUIDs)+len(state.UUIDs) == 0 || ValidLookup(state.XUIDs, state.UUIDs))
}

// SyncEmotes coalesces local updates without waiting for the network and returns unexpired watched playback.
func (c *Client) SyncEmotes(state EmoteState) []RemoteEmote {
	return c.syncEmotes(state, time.Now())
}

// syncEmotes advances one deterministic exchange at the caller's observation time.
func (c *Client) syncEmotes(state EmoteState, now time.Time) []RemoteEmote {
	c.mu.Lock()
	defer c.mu.Unlock()
	s := c.live
	if s == nil || !ValidEmoteState(state) {
		return nil
	}
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
	return result
}

// watches resolves either identity namespace while keeping subscription memory bounded.
func watches(state EmoteState, xuid, uuid string) bool {
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
	if !emoteIDPattern.MatchString(*msg.EmoteID) || msg.ValidForMS > uint64(emoteTTL/time.Millisecond) {
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
