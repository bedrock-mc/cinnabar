package cinnabarapi

import (
	"bytes"
	"encoding/json"
	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/hashimthearab/rust-mcbe/core/internal/testwait"
	"github.com/santhosh-tekuri/jsonschema/v6"
	"strings"
	"testing"
	"time"
)

func TestEmoteExchangeScopesLifetimesAndExpiresWithoutNetwork(t *testing.T) {
	now := time.Unix(123, 0)
	s := &session{emoteDirty: make(chan struct{}, 1), emotes: make(map[string]receivedEmote)}
	c := &Client{live: s}
	state := EmoteState{XUIDs: []string{"11"}, EmoteID: "cinnabar:twerk", PlaybackID: 1, ElapsedMS: 30}
	if got := c.syncEmotes(state, now); len(got) != 0 {
		t.Fatal(got)
	}
	clip := "cinnabar:twerk"
	msg := inbound{Type: "emote", XUID: "11", UUID: "00112233-4455-6677-8899-aabbccddeeff", EmoteID: &clip, ElapsedMS: 42, ValidForMS: uint64(emoteTTL / time.Millisecond)}
	c.receiveEmoteAt(s, msg, now)
	got := c.syncEmotes(state, now.Add(time.Second))
	if len(got) != 1 || got[0].ElapsedMS != 1042 || got[0].ValidForMS != 14000 {
		t.Fatal(got)
	}
	c.receiveEmoteAt(s, inbound{XUID: msg.XUID, UUID: msg.UUID}, now.Add(time.Second))
	if len(c.syncEmotes(state, now.Add(time.Second))) != 0 {
		t.Fatal("stop retained playback")
	}
	c.receiveEmoteAt(s, msg, now)
	if len(c.syncEmotes(state, now.Add(emoteTTL))) != 0 {
		t.Fatal("expired playback")
	}
	state.XUIDs = []string{"22"}
	c.syncEmotes(state, now.Add(emoteTTL+time.Second))
	c.receiveEmoteAt(s, msg, now.Add(emoteTTL+time.Second))
	if len(s.emotes) != 0 {
		t.Fatal("unwatched sender admitted")
	}
	c.live = nil
	if len(c.syncEmotes(state, now)) != 0 {
		t.Fatal("disconnected session retained playback")
	}
}

func TestEmoteUpdatesCoalesceAndRenew(t *testing.T) {
	now := time.Unix(123, 0)
	s := &session{emoteDirty: make(chan struct{}, 1), emotes: make(map[string]receivedEmote)}
	c := &Client{live: s}
	state := EmoteState{EmoteID: "cinnabar:twerk", PlaybackID: 1}
	c.syncEmotes(state, now)
	<-s.emoteDirty
	state.PlaybackID = 2
	c.syncEmotes(state, now.Add(emoteSpacing/2))
	if len(s.emoteDirty) != 0 {
		t.Fatal("burst was not coalesced")
	}
	c.syncEmotes(state, now.Add(emoteSpacing))
	if len(s.emoteDirty) != 1 || s.emoteState.PlaybackID != 2 {
		t.Fatal("restart was lost")
	}
	<-s.emoteDirty
	state.ElapsedMS = 5000
	c.syncEmotes(state, now.Add(emoteTTL/3+emoteSpacing))
	if len(s.emoteDirty) != 1 || s.emoteState.ElapsedMS != 5000 {
		t.Fatal("lease was not renewed")
	}
}

func TestEmoteMessagesMatchSchemaAndBoundNearbySubscriptions(t *testing.T) {
	clip := "cinnabar:twerk"
	validate(t, "ClientMessage", emoteMessage{Type: "emote_state", XUIDs: &[]string{"11"}, EmoteID: &clip, ElapsedMS: 42})
	validate(t, "ClientMessage", emoteMessage{Type: "emote_state"})
	state := EmoteState{XUIDs: make([]string, bridgecontract.CinnabarAPIMaxLookupXUIDs+1)}
	if ValidEmoteState(state) {
		t.Fatal("unbounded subscription")
	}
	state = EmoteState{EmoteID: "not a clip"}
	if ValidEmoteState(state) {
		t.Fatal("malformed clip")
	}
}

func TestEmoteConnectionSendsAndReceivesAuthenticatedPlayback(t *testing.T) {
	api := newFakeAPI(t)
	client := newTestClient(t, api, nil)
	client.Configure(testConfig)
	session := receive(t, api.sessions)
	testwait.Eventually(t, testwait.DefaultTimeout, "authenticated emote session", func() bool { client.mu.Lock(); defer client.mu.Unlock(); return client.live != nil })
	state := EmoteState{XUIDs: []string{"11"}, EmoteID: "cinnabar:twerk", ElapsedMS: 123, PlaybackID: 1}
	client.SyncEmotes(state)
	var sent emoteMessage
	if err := readJSON(t.Context(), session.conn, &sent); err != nil {
		t.Fatal(err)
	}
	if sent.Type != "emote_state" || sent.EmoteID == nil || *sent.EmoteID != state.EmoteID || sent.ElapsedMS != 123 {
		t.Fatal(sent)
	}
	session.send(t, map[string]any{"type": "emote", "xuid": "11", "uuid": "00112233-4455-6677-8899-aabbccddeeff", "emote_id": "cinnabar:twerk", "elapsed_ms": 42, "valid_for_ms": 15000})
	testwait.Eventually(t, testwait.DefaultTimeout, "shared playback", func() bool { return len(client.SyncEmotes(state)) == 1 })
	session.send(t, map[string]any{"type": "emote", "xuid": "11", "uuid": "00112233-4455-6677-8899-aabbccddeeff", "emote_id": nil, "elapsed_ms": 0, "valid_for_ms": 0})
	testwait.Eventually(t, testwait.DefaultTimeout, "shared playback stop", func() bool { return len(client.SyncEmotes(state)) == 0 })
}

func TestEmoteLimitsMatchPinnedSchema(t *testing.T) {
	var schema struct {
		Defs struct {
			Emote struct {
				Properties struct {
					ValidForMS struct {
						Maximum uint64 `json:"maximum"`
					} `json:"valid_for_ms"`
				} `json:"properties"`
			} `json:"Emote"`
		} `json:"$defs"`
	}
	if err := json.Unmarshal(pinnedSchema, &schema); err != nil {
		t.Fatal(err)
	}
	if schema.Defs.Emote.Properties.ValidForMS.Maximum != uint64(emoteTTL/time.Millisecond) {
		t.Fatal("emote lease differs from schema")
	}
	for _, clip := range []string{"a", strings.Repeat("a", 128), strings.Repeat("a", 129), "bad clip"} {
		payload, _ := json.Marshal(emoteMessage{Type: "emote_state", EmoteID: &clip})
		value, err := jsonschema.UnmarshalJSON(bytes.NewReader(payload))
		if err != nil {
			t.Fatal(err)
		}
		accepted := compileDefinition(t, "ClientMessage").Validate(value) == nil
		if accepted != ValidEmoteState(EmoteState{EmoteID: clip}) {
			t.Fatalf("clip validation differs from schema: %d bytes", len(clip))
		}
	}
}

func TestEmoteRefreshesDoNotResendStableWatchLists(t *testing.T) {
	now := time.Unix(123, 0)
	s := &session{emoteDirty: make(chan struct{}, 1), emotes: make(map[string]receivedEmote)}
	c := &Client{live: s}
	state := EmoteState{XUIDs: []string{"11"}, EmoteID: "cinnabar:twerk"}
	c.syncEmotes(state, now)
	var first, renewed emoteMessage
	if err := json.Unmarshal(c.emotePayload(s), &first); err != nil {
		t.Fatal(err)
	}
	if first.XUIDs == nil {
		t.Fatal("initial subscriptions omitted")
	}
	state.ElapsedMS = 5000
	c.syncEmotes(state, now.Add(emoteTTL/3))
	if err := json.Unmarshal(c.emotePayload(s), &renewed); err != nil {
		t.Fatal(err)
	}
	if renewed.XUIDs != nil || renewed.UUIDs != nil {
		t.Fatal("unchanged subscription bytes repeated")
	}
	state.XUIDs = nil
	c.syncEmotes(state, now.Add(emoteTTL))
	if err := json.Unmarshal(c.emotePayload(s), &renewed); err != nil {
		t.Fatal(err)
	}
	if renewed.XUIDs == nil || len(*renewed.XUIDs) != 0 {
		t.Fatal("empty replacement omitted")
	}
}
