package catalog

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strings"
	"testing"

	"github.com/df-mc/go-xsapi/v2/social"
	"github.com/df-mc/go-xsapi/v2/xal/xsts"
)

// friendsClient answers the friends list with people, recording the requested path.
func friendsClient(t *testing.T, people []map[string]any, path *string) *social.Client {
	t.Helper()
	body, err := json.Marshal(map[string]any{"people": people})
	if err != nil {
		t.Fatal(err)
	}
	transport := roundTripFunc(func(req *http.Request) (*http.Response, error) {
		*path = req.URL.Path
		return &http.Response{StatusCode: http.StatusOK, Header: http.Header{"Content-Type": {"application/json"}}, Body: io.NopCloser(strings.NewReader(string(body)))}, nil
	})
	return social.New(&http.Client{Transport: transport}, nil, xsts.UserInfo{}, nil)
}

// Online friends come first, each group by gamertag; presence and gamerpics come from the
// decorated list, and entries the screen cannot show are left out.
func TestFriendPeopleListsPresenceAndGamerpics(t *testing.T) {
	var path string
	people := friendsClient(t, []map[string]any{
		{"xuid": "3", "gamertag": "zed", "presenceState": "Online", "displayPicRaw": "https://images.example.test/z"},
		{"xuid": "1", "gamertag": "Alex", "presenceState": "Offline", "displayPicRaw": "http://insecure.test/a"},
		{"xuid": "2", "displayName": "Bea", "presenceState": "Online"},
		{"xuid": "", "gamertag": "NoXuid", "presenceState": "Online"},
		{"xuid": "4", "presenceState": "Online"},
	}, &path)
	got, err := friendPeople(context.Background(), people)
	if err != nil {
		t.Fatal(err)
	}
	if !strings.HasPrefix(path, "/users/me/people/friends/decoration/") || !strings.Contains(path, "presenceDetail") {
		t.Fatalf("path = %s", path)
	}
	if len(got) != 3 || got[0].Gamertag != "Bea" || got[1].Gamertag != "zed" || got[2].Gamertag != "Alex" {
		t.Fatalf("people = %+v", got)
	}
	if !got[0].Online || !got[1].Online || got[2].Online {
		t.Fatalf("presence = %+v", got)
	}
	if got[2].Gamerpic.URL != "" || got[0].Gamerpic.URL != "" {
		t.Fatalf("unsafe or absent gamerpics must stay empty: %+v", got)
	}
	picture, err := url.Parse(got[1].Gamerpic.URL)
	if err != nil || picture.Host != "images.example.test" || picture.Query().Get("w") != fmt.Sprint(gamerpicSide) {
		t.Fatalf("gamerpic = %q", got[1].Gamerpic.URL)
	}
}

// A long friends list is cut to MaxPeople, dropping offline friends before online ones.
func TestFriendPeopleKeepsOnlineFriendsWithinTheBound(t *testing.T) {
	var path string
	var raw []map[string]any
	for index := range MaxPeople + 5 {
		raw = append(raw, map[string]any{"xuid": fmt.Sprint(index + 1), "gamertag": fmt.Sprintf("offline%03d", index), "presenceState": "Offline"})
	}
	raw = append(raw, map[string]any{"xuid": "9999", "gamertag": "zz-online", "presenceState": "Online"})
	got, err := friendPeople(context.Background(), friendsClient(t, raw, &path))
	if err != nil {
		t.Fatal(err)
	}
	if len(got) != MaxPeople || got[0].Gamertag != "zz-online" || got[len(got)-1].Online {
		t.Fatalf("len = %d first = %+v", len(got), got[0])
	}
}
