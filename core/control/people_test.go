package control

import (
	"context"
	"encoding/json"
	"fmt"
	"reflect"
	"strings"
	"testing"
)

type stubPeople struct {
	stubServices
	people []Person
}

func TestFriendsPeopleSerializedBoundaries(t *testing.T) {
	const id = ^uint64(0)
	first := Person{XUID: "1", Gamertag: "first", Online: true}
	boundary := []Person{first, {XUID: "2", Gamertag: "last", Gamerpic: Image{URL: "x"}}}
	encoded, err := json.Marshal(peopleResultV1{SchemaVersion: 1, Friends: boundary})
	if err != nil {
		t.Fatal(err)
	}
	// Fill the result budget using actual JSON sizes, including the optional URL field.
	boundary[1].Gamerpic.URL = strings.Repeat("x", frameBudget-len(encoded)+1)
	overBoundary := append([]Person{}, boundary...)
	overBoundary[1].Gamerpic.URL += "x"
	oversized := Person{XUID: "3", Gamertag: strings.Repeat("<\"\\", MaxFrameLen)}
	for _, tc := range []struct {
		name   string
		people []Person
		want   []Person
	}{
		{name: "nil", want: []Person{}},
		{name: "empty", people: []Person{}, want: []Person{}},
		{name: "at result budget", people: boundary, want: boundary},
		{name: "one byte over result budget", people: overBoundary, want: []Person{first}},
		{name: "oversized only friend", people: []Person{oversized}, want: []Person{}},
		{name: "escaped oversized trailing friend", people: []Person{first, oversized}, want: []Person{first}},
	} {
		t.Run(tc.name, func(t *testing.T) {
			dir := startServices(t, NewStore(), &stubPeople{people: tc.people})
			raw := exchange(t, dir, []byte(fmt.Sprintf(
				`{"jsonrpc":"2.0","id":%d,"method":%q}`, id, methodFriendsPeople)))
			if len(raw) > MaxFrameLen {
				t.Fatalf("reply with envelope = %d bytes", len(raw))
			}
			var reply struct {
				JSONRPC string         `json:"jsonrpc"`
				ID      uint64         `json:"id"`
				Result  peopleResultV1 `json:"result"`
				Error   *responseError `json:"error"`
			}
			if err := json.Unmarshal(raw, &reply); err != nil {
				t.Fatal(err)
			}
			if reply.JSONRPC != "2.0" || reply.ID != id || reply.Error != nil || reply.Result.SchemaVersion != 1 {
				t.Fatalf("invalid reply envelope: version %q, id %d, error %+v, schema %d",
					reply.JSONRPC, reply.ID, reply.Error, reply.Result.SchemaVersion)
			}
			if !reflect.DeepEqual(reply.Result.Friends, tc.want) {
				t.Fatalf("kept %d friends, want %d with their original values", len(reply.Result.Friends), len(tc.want))
			}
		})
	}
}

func (s *stubPeople) People(context.Context) ([]Person, error) { return s.people, s.err }

func TestFriendsPeopleListsTheAccountsFriends(t *testing.T) {
	stub := &stubPeople{people: []Person{
		{XUID: "2535400000000001", Gamertag: "Alex", Online: true, Gamerpic: Image{URL: "https://a.test/p", Path: "/art/people/p.img"}},
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
	people := make([]Person, 110)
	for index := range people {
		people[index] = Person{
			XUID: fmt.Sprint(2535400000000000 + index), Gamertag: fmt.Sprintf("friend%03d", index), Online: index < 3,
			Gamerpic: Image{URL: "https://images.example.test/image?url=" + strings.Repeat("u", 400), Path: "/art/people/" + strings.Repeat("p", 200)},
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
	if len(friends) == 0 || len(friends) >= len(people) || friends[0] != people[0] || friends[len(friends)-1] != people[len(friends)-1] {
		t.Fatalf("kept %d friends", len(friends))
	}
}
