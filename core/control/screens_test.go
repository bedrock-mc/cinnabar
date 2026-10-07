package control

import (
	"context"
	"encoding/json"
	"strings"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/catalog"
)

type stubScreens struct {
	stubServices
}

type recordingScreens struct {
	stubScreens
	events []catalog.MessageEvent
}

// ReportMessage records the reports delivered by the control endpoint.
func (s *recordingScreens) ReportMessage(_ context.Context, event catalog.MessageEvent) error {
	s.events = append(s.events, event)
	return nil
}

func TestMessageEndpointForwardsBulkDeleteAndSubsequentReports(t *testing.T) {
	screens := &recordingScreens{}
	dir := startServices(t, NewStore(), screens)
	for _, params := range []string{
		`{"event_type":"DeleteAllRead"}`,
		`{"event_type":"Click","instance_id":"next","report_id":"report"}`,
	} {
		if reply := rpc(t, dir, methodMessageEvent, params); reply.Error != nil {
			t.Fatalf("message event %s was rejected: %+v", params, reply.Error)
		}
	}
	if len(screens.events) != 2 || screens.events[0] != (catalog.MessageEvent{Type: "DeleteAllRead"}) ||
		screens.events[1] != (catalog.MessageEvent{Type: "Click", InstanceID: "next", ReportID: "report"}) {
		t.Fatalf("forwarded events = %+v", screens.events)
	}
}

func (stubScreens) FeaturedServers(context.Context) ([]catalog.FeaturedServer, error) {
	return []catalog.FeaturedServer{{Name: "Example", Address: "play.example.test:19132"}}, nil
}

func (stubScreens) Profile(context.Context) (catalog.Profile, error) {
	return catalog.Profile{Gamertag: "Steve", XUID: "1"}, nil
}

func (stubScreens) Home(context.Context) (catalog.Home, error) {
	return catalog.Home{RealmInvites: 2, Messages: []catalog.Message{}}, nil
}

func (stubScreens) ReportMessage(context.Context, catalog.MessageEvent) error { return nil }

func (stubScreens) Ping(_ context.Context, addresses []string) []catalog.PingResult {
	results := make([]catalog.PingResult, len(addresses))
	for index, address := range addresses {
		results[index] = catalog.PingResult{Address: address, Online: true, Players: 3}
	}
	return results
}

func TestScreenFeedsServeWhenTheBackendSupportsThem(t *testing.T) {
	dir := startServices(t, NewStore(), &stubScreens{})
	var featured featuredServersResultV1
	if reply := rpc(t, dir, methodFeaturedServers, ""); reply.Error != nil || json.Unmarshal(reply.Result, &featured) != nil ||
		len(featured.Servers) != 1 || featured.Servers[0].Address != "play.example.test:19132" {
		t.Fatalf("featured = %+v / %+v", featured, reply.Error)
	}
	var profile profileResultV1
	if reply := rpc(t, dir, methodProfile, ""); reply.Error != nil || json.Unmarshal(reply.Result, &profile) != nil ||
		profile.Profile.Gamertag != "Steve" {
		t.Fatalf("profile = %+v / %+v", profile, reply.Error)
	}
	if reply := rpc(t, dir, methodProfile, `{"x":1}`); reply.Error == nil || reply.Error.Code != -32602 {
		t.Fatalf("params must be rejected: %+v", reply.Error)
	}
	var pinged pingResultV1
	if reply := rpc(t, dir, methodPing, `{"addresses":["a.test:19132"]}`); reply.Error != nil ||
		json.Unmarshal(reply.Result, &pinged) != nil || len(pinged.Servers) != 1 || pinged.Servers[0].Players != 3 {
		t.Fatalf("ping = %+v / %+v", pinged, reply.Error)
	}
	if reply := rpc(t, dir, methodPing, ""); reply.Error == nil || reply.Error.Code != -32602 {
		t.Fatalf("ping needs addresses: %+v", reply.Error)
	}
	var home homeResultV1
	if reply := rpc(t, dir, methodHome, ""); reply.Error != nil || json.Unmarshal(reply.Result, &home) != nil ||
		home.Home.RealmInvites != 2 {
		t.Fatalf("home = %+v / %+v", home, reply.Error)
	}
	if reply := rpc(t, dir, methodMessageEvent, `{"event_type":"Click","instance_id":"i","report_id":"r","button_id":"b"}`); reply.Error != nil {
		t.Fatalf("event = %+v", reply.Error)
	}
	if reply := rpc(t, dir, methodMessageEvent, `{"event_type":"Hack"}`); reply.Error == nil || reply.Error.Code != -32602 {
		t.Fatalf("unknown events must be rejected: %+v", reply.Error)
	}
}

func TestScreenFeedsNeedAScreenBackend(t *testing.T) {
	dir := startServices(t, NewStore(), &stubServices{})
	if reply := rpc(t, dir, methodFeaturedServers, ""); reply.Error == nil || reply.Error.Code != codeServicesDisabled {
		t.Fatalf("reply = %+v", reply.Error)
	}
}

// countedScreens records whether the local caller requested the experience-details data.
type countedScreens struct {
	stubScreens
	calls int
}

// FeaturedServersWithCounts supplies a real zero without losing its presence on the bridge.
func (s *countedScreens) FeaturedServersWithCounts(context.Context) ([]catalog.FeaturedServer, error) {
	s.calls++
	count := int64(0)
	return []catalog.FeaturedServer{{Name: "Experience", PlayerCount: &count}}, nil
}

// TestFeaturedCountsAreOptIn keeps background catalog reads from requesting populations.
func TestFeaturedCountsAreOptIn(t *testing.T) {
	s := new(countedScreens)
	for _, params := range []string{"", `{}`, `{"include_player_counts":false}`} {
		if _, err := screenResult(context.Background(), s, methodFeaturedServers, json.RawMessage(params)); err != nil {
			t.Fatal(err)
		}
	}
	if s.calls != 0 {
		t.Fatal("ordinary featured lookup requested player counts")
	}
	result, err := screenResult(context.Background(), s, methodFeaturedServers, json.RawMessage(`{"include_player_counts":true}`))
	if err != nil || s.calls != 1 {
		t.Fatalf("count request: calls=%d, error=%v", s.calls, err)
	}
	raw, err := json.Marshal(result)
	if err != nil || !strings.Contains(string(raw), `"player_count":0`) {
		t.Fatalf("count result = %s, error=%v", raw, err)
	}
}
