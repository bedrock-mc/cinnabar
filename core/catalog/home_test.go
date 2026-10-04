package catalog

import (
	"encoding/json"
	"testing"
	"time"

	"github.com/sandertv/gophertunnel/minecraft/service/gatherings"
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
func TestLiveEventsKeepRunningOnes(t *testing.T) {
	var configs []gatherings.GatheringConfig
	data := []byte(`[
		{"gatheringId":"g1","title":"Live","startTimeUtc":"2026-09-01T00:00:00Z","endTimeUtc":"2026-12-01T00:00:00Z",
		 "externalVenue":{"serverIpAddress":"1.2.3.4","serverPort":19132},
		 "segments":[{"startTimeUtc":"2026-08-01T00:00:00Z","endTimeUtc":"2026-08-02T00:00:00Z","ui":{"startScreenButtonText":"Soon"}},
		             {"startTimeUtc":"2026-09-01T00:00:00Z","ui":{"startScreenButtonText":"Watch",
		   "captionText":"Live now","captionIncludesCountdown":true,"badgeImage":"https://cdn.test/b.png"}}]},
		{"gatheringId":"old","endTimeUtc":"2020-01-01T00:00:00Z"}
	]`)
	if err := json.Unmarshal(data, &configs); err != nil {
		t.Fatal(err)
	}
	events := liveEventsFrom(configs, time.Date(2026, 9, 29, 0, 0, 0, 0, time.UTC))
	if len(events) != 1 {
		t.Fatalf("events = %+v", events)
	}
	event := events[0]
	if event.Address != "1.2.3.4:19132" || event.ButtonText != "Watch" || !event.CaptionCountdown || event.Badge.URL == "" {
		t.Fatalf("event = %+v", event)
	}
}

// A partial refresh keeps the previous copy of each part that failed, and only those.
func TestRefillKeepsOnlyFailedParts(t *testing.T) {
	previous := Home{RealmInvites: 2, LiveEvents: []LiveEvent{{ID: "old"}}, Messages: []Message{{ID: "old"}}}
	fresh := Home{RealmInvites: 5, Messages: []Message{{ID: "new"}}, failed: partEvents | partPersona}
	got := fresh.Refill(previous)
	if got.RealmInvites != 5 || got.Messages[0].ID != "new" || len(got.LiveEvents) != 1 || got.LiveEvents[0].ID != "old" {
		t.Fatalf("refilled = %+v", got)
	}
	if fresh.Failed() || !(Home{failed: allHomeParts}).Failed() {
		t.Fatal("Failed must mean every part failed")
	}
}
