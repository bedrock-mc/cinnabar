package control

import (
	"context"
	"encoding/json"
	"fmt"
	"strings"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/catalog"
)

type stubPeople struct {
	stubServices
	people []catalog.Person
}

func (s *stubPeople) People(context.Context) ([]catalog.Person, error) { return s.people, s.err }

func TestFriendsPeopleListsTheAccountsFriends(t *testing.T) {
	stub := &stubPeople{people: []catalog.Person{
		{XUID: "2535400000000001", Gamertag: "Alex", Online: true, Gamerpic: catalog.Image{URL: "https://a.test/p", Path: "/art/people/p.img"}},
	}}
	dir := startServices(t, NewStore(), stub)
	var result peopleResultV1
	if reply := rpc(t, dir, methodFriendsPeople, ""); reply.Error != nil || json.Unmarshal(reply.Result, &result) != nil ||
		result.SchemaVersion != 1 || len(result.Friends) != 1 || result.Friends[0] != stub.people[0] {
		t.Fatalf("people = %+v / %+v", result, reply.Error)
	}
	if reply := rpc(t, dir, methodFriendsPeople, `{"xuid":"1"}`); reply.Error == nil || reply.Error.Code != -32602 {
		t.Fatalf("params must be rejected: %+v", reply.Error)
	}
	stub.people = nil
	if raw := string(call(t, dir, methodFriendsPeople, "")); !strings.Contains(raw, `"friends":[]`) {
		t.Fatalf("empty people response = %s", raw)
	}
	stub.err = ErrSignedOut
	if reply := rpc(t, dir, methodFriendsPeople, ""); reply.Error == nil || reply.Error.Code != codeSignedOut {
		t.Fatalf("signed out = %+v", reply.Error)
	}
}

func TestFriendsPeopleNeedsAPeopleBackend(t *testing.T) {
	dir := startServices(t, NewStore(), &stubServices{})
	if reply := rpc(t, dir, methodFriendsPeople, ""); reply.Error == nil || reply.Error.Code != codeServicesDisabled {
		t.Fatalf("reply = %+v", reply.Error)
	}
}

// A list too large for one frame loses its trailing (offline) friends rather than failing.
func TestFriendsPeopleFitsOneFrame(t *testing.T) {
	people := make([]catalog.Person, catalog.MaxPeople+10)
	for index := range people {
		people[index] = catalog.Person{
			XUID: fmt.Sprint(2535400000000000 + index), Gamertag: fmt.Sprintf("friend%03d", index), Online: index < 3,
			Gamerpic: catalog.Image{URL: "https://images.example.test/image?url=" + strings.Repeat("u", 400), Path: "/art/people/" + strings.Repeat("p", 200)},
		}
	}
	dir := startServices(t, NewStore(), &stubPeople{people: people})
	raw := call(t, dir, methodFriendsPeople, "")
	var reply struct {
		Result peopleResultV1 `json:"result"`
	}
	if err := json.Unmarshal(raw, &reply); err != nil || len(raw) > MaxFrameLen {
		t.Fatalf("reply of %d bytes: %v", len(raw), err)
	}
	friends := reply.Result.Friends
	if len(friends) == 0 || len(friends) >= catalog.MaxPeople || friends[0] != people[0] || friends[len(friends)-1] != people[len(friends)-1] {
		t.Fatalf("kept %d friends", len(friends))
	}
}
