package catalog

import (
	"encoding/json"
	"testing"

	"github.com/sandertv/gophertunnel/minecraft/service/playermessaging"
)

// Authored to the vanilla field names; not a captured payload.
const messagingFixture = `{"result":{
	"continuationToken":"c2",
	"messages":[
		{"id":"m1","instanceId":"i1","surface":"PlayButton","template":"ImageTile",
		 "messageText":{"header":"New!"},"images":{"tile":{"url":"https://cdn.test/t.png"}},
		 "buttons":{"go":{"text":"Play","link":"/play","action":"Internal"}}},
		{"id":"","surface":"PlayButton","template":"x"},
		{"id":"m2","surface":"ToastNotification"}
	],
	"inboxSummary":{"totalNumberOfMessages":3,"categories":[
		{"totalNumberOfMessages":3,"totalNumberOfUnreadMessages":2,
		 "categoryInfo":{"type":"News","name":"News"},
		 "messages":[{"id":"m1","instanceId":"i1","surface":"PlayButton","template":"ImageTile"},
		             {"id":"m3","instanceId":"i3","surface":"InboxMessage","template":"Text","status":"Unread"}]}
	]}
}}`

func TestMessagesKeepWellFormedEntriesOnce(t *testing.T) {
	var envelope struct {
		Result playermessaging.Session `json:"result"`
	}
	if err := json.Unmarshal([]byte(messagingFixture), &envelope); err != nil {
		t.Fatal(err)
	}
	messages, inbox := flatten(&envelope.Result)
	if len(messages) != 2 || messages[0].ID != "m1" || messages[1].Surface != "InboxMessage" {
		t.Fatalf("messages = %+v", messages)
	}
	if messages[0].Buttons[0].Action != "internal" || messages[0].Images[0].URL != "https://cdn.test/t.png" {
		t.Fatalf("parts = %+v", messages[0])
	}
	if inbox.Total != 3 || inbox.Unread != 2 || inbox.Categories[0].Type != "News" {
		t.Fatalf("inbox = %+v", inbox)
	}
}

// Ended events are dropped and the running segment dresses the button.
// A partial refresh keeps the previous copy of each part that failed, and only those.
func TestRefillKeepsOnlyFailedParts(t *testing.T) {
	previous := Home{RealmInvites: 2, PersonaHead: Image{Path: "/old"}, Messages: []Message{{ID: "old"}}}
	fresh := Home{RealmInvites: 5, Messages: []Message{{ID: "new"}}, failed: partPersona}
	got := fresh.Refill(previous)
	if got.RealmInvites != 5 || got.Messages[0].ID != "new" || got.PersonaHead.Path != "/old" {
		t.Fatalf("refilled = %+v", got)
	}
	if fresh.Failed() || !(Home{failed: allHomeParts}).Failed() {
		t.Fatal("Failed must mean every part failed")
	}
}

// The game no longer fetches live events, so the home feed neither carries nor waits on them.
func TestHomeHasNoLiveEvents(t *testing.T) {
	if !(Home{failed: partInvites | partTreatments | partMessages | partPersona}).Failed() {
		t.Fatal("a home whose service parts all failed must count as failed")
	}
	var wire map[string]json.RawMessage
	data, _ := json.Marshal(Home{})
	if err := json.Unmarshal(data, &wire); err != nil {
		t.Fatal(err)
	}
	if _, ok := wire["live_events"]; ok {
		t.Fatal("home feed still sends live events")
	}
}
