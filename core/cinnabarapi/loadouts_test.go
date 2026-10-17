package cinnabarapi

import (
	"encoding/json"
	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/hashimthearab/rust-mcbe/core/internal/testwait"
	"testing"
	"time"
)

// selection supplies one supported namespaced item for protocol tests.
func selection() []bridgecontract.CinnabarCosmeticSlot {
	return []bridgecontract.CinnabarCosmeticSlot{{Slot: "head", ItemID: "cinnabar:bunny_ears"}}
}

func TestLoadoutsPersistWithoutLeasesAndRejectStaleOrUnwatchedUpdates(t *testing.T) {
	now := time.Unix(123, 0)
	s := &session{emoteDirty: make(chan struct{}, 1), loadoutDirty: make(chan struct{}, 1), emotes: make(map[string]receivedEmote)}
	c := &Client{live: s}
	state := PlayerState{XUIDs: []string{"11"}, Slots: selection()}
	c.syncPlayers(state, now)
	id := "cinnabar:bunny_ears"
	message := inbound{Type: typeLoadout, XUID: "11", UUID: "00112233-4455-6677-8899-aabbccddeeff", Revision: 5, Slots: map[string]*string{"head": &id}}
	c.receiveLoadout(s, message)
	if got := c.syncPlayers(state, now.Add(24*time.Hour)).Loadouts; len(got) != 1 || got[0].Slots[0].ItemID != id {
		t.Fatal(got)
	}
	message.Revision = 4
	message.Slots = map[string]*string{}
	c.receiveLoadout(s, message)
	if len(s.loadouts["11"].Slots) != 1 {
		t.Fatal("stale withdrawal replaced newer loadout")
	}
	message.Revision = 5
	c.receiveLoadout(s, message)
	if len(s.loadouts["11"].Slots) != 0 {
		t.Fatal("same-revision withdrawal was lost")
	}
	state.XUIDs = nil
	if len(c.syncPlayers(state, now).Loadouts) != 0 {
		t.Fatal("departed peer retained")
	}
	c.receiveLoadout(s, message)
	if len(s.loadouts) != 0 {
		t.Fatal("unwatched identity admitted")
	}
	c.live = nil
	if len(c.syncPlayers(state, now).Loadouts) != 0 {
		t.Fatal("disconnected session retained")
	}
}

func TestLoadoutsUseIndependentRevisionedSchemaValidMessages(t *testing.T) {
	s := &session{loadoutDirty: make(chan struct{}, 1)}
	c := &Client{}
	now := time.Unix(123, 0)
	c.queueLoadout(s, selection(), now)
	payload := c.loadoutPayload(s)
	validate(t, "ClientMessage", json.RawMessage(payload))
	first := s.loadoutRevision
	c.queueLoadout(s, selection(), now.Add(time.Hour))
	if s.loadoutRevision != first {
		t.Fatal("unchanged loadout renewed as an emote lease")
	}
	c.queueLoadout(s, nil, now)
	if s.loadoutRevision <= first {
		t.Fatal("revision went backwards")
	}
	validate(t, "ClientMessage", json.RawMessage(c.loadoutPayload(s)))
	for _, slots := range [][]bridgecontract.CinnabarCosmeticSlot{{{Slot: "head", ItemID: "bad id"}}, {{Slot: "head", ItemID: "cinnabar:bunny_ears"}, {Slot: "head", ItemID: "cinnabar:bunny_ears"}}} {
		if ValidPlayerState(PlayerState{Slots: slots}) {
			t.Fatal("invalid selection admitted")
		}
	}
}

func TestLoadoutConnectionWritesAndReceivesIndependentSnapshots(t *testing.T) {
	api := newFakeAPI(t)
	client := newTestClient(t, api, nil)
	client.Configure(testConfig)
	session := receive(t, api.sessions)
	testwait.Eventually(t, testwait.DefaultTimeout, "authenticated loadout session", func() bool { client.mu.Lock(); defer client.mu.Unlock(); return client.live != nil })
	state := PlayerState{XUIDs: []string{"11"}, Slots: selection()}
	client.SyncPlayers(state)
	found := false
	for range 2 {
		var message map[string]any
		if err := readJSON(t.Context(), session.conn, &message); err != nil {
			t.Fatal(err)
		}
		validate(t, "ClientMessage", message)
		if message["type"] == typeLoadout {
			found = true
			if message["slots"].(map[string]any)["head"] != state.Slots[0].ItemID {
				t.Fatal(message)
			}
		}
	}
	if !found {
		t.Fatal("loadout was not written independently")
	}
	session.send(t, map[string]any{"type": typeLoadout, "xuid": "11", "uuid": "00112233-4455-6677-8899-aabbccddeeff", "revision": 9, "slots": map[string]any{"head": state.Slots[0].ItemID}})
	testwait.Eventually(t, testwait.DefaultTimeout, "received loadout", func() bool { got := client.SyncPlayers(state).Loadouts; return len(got) == 1 && len(got[0].Slots) == 1 })
	session.send(t, map[string]any{"type": typeLoadout, "xuid": "11", "uuid": "00112233-4455-6677-8899-aabbccddeeff", "revision": 9, "slots": map[string]any{}})
	testwait.Eventually(t, testwait.DefaultTimeout, "loadout withdrawal", func() bool { return len(client.SyncPlayers(state).Loadouts) == 0 })
}

func TestLoadoutWithdrawalAllowsTheNextSessionToRestartItsRevision(t *testing.T) {
	s := &session{emoteState: PlayerState{XUIDs: []string{"11"}}}
	c := &Client{live: s}
	id := "cinnabar:bunny_ears"
	message := inbound{Type: typeLoadout, XUID: "11", UUID: "00112233-4455-6677-8899-aabbccddeeff", Revision: 10, Slots: map[string]*string{"head": &id}}
	c.receiveLoadout(s, message)
	message.Slots = map[string]*string{}
	c.receiveLoadout(s, message)
	message.Revision = 1
	message.Slots = map[string]*string{"head": &id}
	c.receiveLoadout(s, message)
	got := c.syncPlayers(PlayerState{XUIDs: []string{"11"}}, time.Unix(123, 0)).Loadouts
	if len(got) != 1 || len(got[0].Slots) != 1 || got[0].Revision != message.Revision {
		t.Fatalf("withdrawn session revision blocked the next session: %+v", got)
	}
}
