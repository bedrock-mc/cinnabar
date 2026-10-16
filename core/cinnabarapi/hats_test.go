package cinnabarapi

import (
	"encoding/json"
	"strings"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/testwait"
)

func TestSharedHatLeasesTrackSubscriptionsAndExplicitRemoval(t *testing.T) {
	now := time.Unix(123, 0)
	s := &session{emoteDirty: make(chan struct{}, 1), emotes: make(map[string]receivedEmote)}
	c := &Client{live: s}
	state := PlayerState{XUIDs: []string{"11"}, HatID: "crown"}
	c.syncPlayers(state, now)
	id := "bunny_ears"
	message := inbound{Type: typeCosmetic, XUID: "11", UUID: "00112233-4455-6677-8899-aabbccddeeff", HatID: &id, ValidForMS: 15000}
	c.receiveHatAt(s, message, now)
	hats := c.syncPlayers(state, now.Add(time.Second)).Hats
	if len(hats) != 1 || hats[0].HatID != id || hats[0].ValidForMS != 14000 {
		t.Fatal(hats)
	}
	c.receiveHatAt(s, inbound{XUID: message.XUID, UUID: message.UUID}, now)
	if len(c.syncPlayers(state, now).Hats) != 0 {
		t.Fatal("unequipped hat retained")
	}
	c.receiveHatAt(s, message, now)
	if len(c.syncPlayers(state, now.Add(emoteTTL)).Hats) != 0 {
		t.Fatal("expired hat retained")
	}
	state.XUIDs = nil
	c.syncPlayers(state, now.Add(emoteTTL))
	c.receiveHatAt(s, message, now.Add(emoteTTL))
	if len(c.syncPlayers(state, now.Add(emoteTTL)).Hats) != 0 {
		t.Fatal("departed player retained")
	}
	c.live = nil
	if len(c.syncPlayers(state, now).Hats) != 0 {
		t.Fatal("disconnected session retained")
	}
}

func TestSharedHatMessagesConformAndRejectMalformedSelections(t *testing.T) {
	id := "crown"
	validate(t, "ClientMessage", emoteMessage{Type: typeEmoteState, HatID: &id})
	validate(t, "ServerMessage", map[string]any{"type": typeCosmetic, "xuid": "11", "uuid": "00112233-4455-6677-8899-aabbccddeeff", "hat_id": id, "valid_for_ms": 15000})
	for _, id := range []string{"a", strings.Repeat("a", 128), strings.Repeat("a", 129), "bad hat", "UPPER"} {
		accepted := compileDefinition(t, "CosmeticId").Validate(id) == nil
		if accepted != ValidPlayerState(PlayerState{HatID: id}) {
			t.Fatalf("hat validation differs from schema: %d bytes", len(id))
		}
	}
	if ValidPlayerState(PlayerState{HatID: "bad hat"}) {
		t.Fatal("malformed hat admitted")
	}
	s := &session{emoteState: PlayerState{XUIDs: []string{"11"}}}
	c := &Client{live: s}
	for _, msg := range []inbound{
		{XUID: "22", UUID: "00112233-4455-6677-8899-aabbccddeeff", HatID: &id, ValidForMS: 15000},
		{XUID: "11", UUID: "invalid", HatID: &id, ValidForMS: 15000},
		{XUID: "11", UUID: "00112233-4455-6677-8899-aabbccddeeff", HatID: &id, ValidForMS: 15001},
	} {
		c.receiveHatAt(s, msg, time.Unix(123, 0))
	}
	if len(s.hats) != 0 {
		t.Fatal("invalid lease admitted")
	}
}

func TestSharedHatConnectionUsesTheAuthenticatedPlayerExchange(t *testing.T) {
	api := newFakeAPI(t)
	client := newTestClient(t, api, nil)
	client.Configure(testConfig)
	session := receive(t, api.sessions)
	testwait.Eventually(t, testwait.DefaultTimeout, "authenticated cosmetics session", func() bool { client.mu.Lock(); defer client.mu.Unlock(); return client.live != nil })
	state := PlayerState{XUIDs: []string{"11"}, HatID: "crown"}
	client.SyncPlayers(state)
	var sent emoteMessage
	if err := readJSON(t.Context(), session.conn, &sent); err != nil {
		t.Fatal(err)
	}
	if sent.HatID == nil || *sent.HatID != state.HatID {
		t.Fatal(sent)
	}
	encoded, _ := json.Marshal(sent)
	validate(t, "ClientMessage", json.RawMessage(encoded))
	session.send(t, map[string]any{"type": typeCosmetic, "xuid": "11", "uuid": "00112233-4455-6677-8899-aabbccddeeff", "hat_id": "bunny_ears", "valid_for_ms": 15000})
	testwait.Eventually(t, testwait.DefaultTimeout, "shared hat", func() bool { return len(client.SyncPlayers(state).Hats) == 1 })
	session.send(t, map[string]any{"type": typeCosmetic, "xuid": "11", "uuid": "00112233-4455-6677-8899-aabbccddeeff", "hat_id": nil, "valid_for_ms": 0})
	testwait.Eventually(t, testwait.DefaultTimeout, "shared hat removal", func() bool { return len(client.SyncPlayers(state).Hats) == 0 })
}
