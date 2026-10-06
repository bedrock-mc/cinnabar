package catalog

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	playfabcatalog "github.com/df-mc/go-playfab/v2/catalog"
	"github.com/df-mc/go-xsapi/v2/social"
	"github.com/df-mc/go-xsapi/v2/xal/xsts"
	"github.com/hashimthearab/rust-mcbe/core/internal/imagecache"
	"github.com/sandertv/gophertunnel/minecraft/service"
	"github.com/sandertv/gophertunnel/minecraft/service/gatherings"
)

// Authored to the open-source catalog item shape; not a captured payload.
const featuredItemFixture = `{
	"Id": "item-1",
	"Title": {"NEUTRAL": "Example Network"},
	"Description": {"NEUTRAL": " A test server. "},
	"Tags": ["pvp"],
	"Images": [
		{"Id": "a", "Tag": "logo", "Type": "thumbnail", "Url": "https://cdn.example.test/logo.png"},
		{"Id": "b", "Tag": "shot", "Type": "screenshot", "Url": "https://cdn.example.test/shot.png"},
		{"Id": "c", "Tag": "game", "Type": "thumbnail", "Url": "http://insecure.example.test/game.png"}
	],
	"DisplayProperties": {
		"url": "play.example.test", "port": 19132, "creatorName": "Example",
		"news": " Season two ", "newsTitle": "News", "unknownField": [1, 2],
		"availableGames": [
			{"title": "Skywars", "subtitle": "Solo", "description": "Fight", "imageTag": "game"},
			{"title": "", "subtitle": ""}
		]
	}
}`

func parseFeatured(t *testing.T, raw string) *gatherings.FeaturedServer {
	t.Helper()
	var item playfabcatalog.Item
	if err := json.Unmarshal([]byte(raw), &item); err != nil {
		t.Fatal(err)
	}
	server, err := gatherings.NewClient(nil).ParseFeaturedServer(item)
	if err != nil {
		t.Fatal(err)
	}
	return server
}

func TestFeaturedServersCarryTheInfoPanelDetails(t *testing.T) {
	servers := featuredServers([]*gatherings.FeaturedServer{parseFeatured(t, featuredItemFixture), nil})
	if len(servers) != 1 {
		t.Fatalf("servers = %+v", servers)
	}
	server := servers[0]
	if server.Name != "Example Network" || server.Address != "play.example.test:19132" || server.Caption != "Skywars" {
		t.Fatalf("identity = %+v", server)
	}
	if server.Description != "A test server." || server.News != "Season two" || server.NewsTitle != "News" {
		t.Fatalf("text = %+v", server)
	}
	if server.Logo.URL != "https://cdn.example.test/logo.png" || len(server.Screenshots) != 1 {
		t.Fatalf("art = %+v", server)
	}
	if len(server.Games) != 1 || server.Games[0].Image.URL != "" {
		t.Fatalf("games keep only titled entries and HTTPS art: %+v", server.Games)
	}
}

func TestFeaturedServersSkipEntriesWithoutAnAddress(t *testing.T) {
	servers := featuredServers([]*gatherings.FeaturedServer{parseFeatured(t, `{"Id": "x", "DisplayProperties": {}}`)})
	if len(servers) != 0 {
		t.Fatalf("servers = %+v", servers)
	}
}

func TestArtworkPruningKeepsTheNewestFiles(t *testing.T) {
	directory := t.TempDir()
	for index, name := range []string{"a.img", "b.img", "c.img"} {
		path := filepath.Join(directory, name)
		if err := os.WriteFile(path, []byte("x"), 0o600); err != nil {
			t.Fatal(err)
		}
		stamp := time.Unix(int64(1000+index), 0)
		if err := os.Chtimes(path, stamp, stamp); err != nil {
			t.Fatal(err)
		}
	}
	cfg := artworkPolicy
	cfg.MaxFiles = 2
	imagecache.New(directory, cfg).Prune()
	if _, err := os.Stat(filepath.Join(directory, "a.img")); !os.IsNotExist(err) {
		t.Fatalf("the oldest file survived: %v", err)
	}
	if _, err := os.Stat(filepath.Join(directory, "c.img")); err != nil {
		t.Fatalf("the newest file was pruned: %v", err)
	}
}

func TestFeaturedImagesPointIntoTheServers(t *testing.T) {
	servers := []FeaturedServer{{Screenshots: []Image{{URL: "https://a.test/s.png"}}, Games: []Game{{}}}}
	images := FeaturedImages(servers)
	if len(images) != 3 {
		t.Fatalf("images = %d", len(images))
	}
	images[1].Path = "/cache/s.img"
	if servers[0].Screenshots[0].Path != "/cache/s.img" {
		t.Fatal("paths must land in the servers")
	}
}

type recordingTransport struct{ hosts []string }

func (r *recordingTransport) RoundTrip(req *http.Request) (*http.Response, error) {
	r.hosts = append(r.hosts, req.URL.Host)
	return nil, errors.New("offline test")
}

type fixedTokens struct{}

func (fixedTokens) ServiceToken(context.Context) (*service.Token, error) {
	return &service.Token{AuthorizationHeader: "MCToken synthetic", ValidUntil: time.Now().Add(time.Hour)}, nil
}

// The gatherings client talks to the discovered endpoint and never falls back to a hardcoded host.
func TestGatheringsClientUsesTheDiscoveredEndpoint(t *testing.T) {
	if _, err := gatheringsClient(&service.Discovery{}, fixedTokens{}); err == nil {
		t.Fatal("undiscovered gatherings service built a client")
	}
	recorder := new(recordingTransport)
	previous := http.DefaultClient.Transport
	http.DefaultClient.Transport = recorder
	t.Cleanup(func() { http.DefaultClient.Transport = previous })
	client, err := gatheringsClient(&service.Discovery{ServiceEnvironments: map[string]map[string]json.RawMessage{
		"gatherings": {"prod": json.RawMessage(`{"serviceUri":"https://gatherings.discovered.example"}`)},
	}}, fixedTokens{})
	if err != nil {
		t.Fatal(err)
	}
	_, _ = client.FeaturedServers(context.Background())
	if len(recorder.hosts) == 0 || recorder.hosts[0] != "gatherings.discovered.example" {
		t.Fatalf("requested hosts = %v", recorder.hosts)
	}
}

// A failed count is omitted from the wire rather than sent as zero.
func TestProfileOmitsUnavailableCounts(t *testing.T) {
	zero := 0
	raw, err := json.Marshal(Profile{Gamertag: "Steve", Friends: &zero})
	if err != nil {
		t.Fatal(err)
	}
	got := string(raw)
	if !strings.Contains(got, `"friends":0`) || strings.Contains(got, "followers") || strings.Contains(got, "gamerscore") {
		t.Fatalf("profile = %s", got)
	}
}

// The profile counts friends and followers from undecorated lists, as the game requests them.
func TestProfileCountsUndecoratedPeopleLists(t *testing.T) {
	var paths []string
	transport := roundTripFunc(func(req *http.Request) (*http.Response, error) {
		paths = append(paths, req.URL.Path)
		body := `{"people":[{"xuid":"1"},{"xuid":"2"},{"xuid":"3"}]}`
		return &http.Response{StatusCode: http.StatusOK, Header: http.Header{"Content-Type": {"application/json"}}, Body: io.NopCloser(strings.NewReader(body))}, nil
	})
	people := social.New(&http.Client{Transport: transport}, nil, xsts.UserInfo{}, nil)
	friends, err := peopleCount(context.Background(), people, social.PeopleListFriends)
	if err != nil || friends != 3 {
		t.Fatalf("friends = %d err = %v", friends, err)
	}
	if followers, err := peopleCount(context.Background(), people, social.PeopleListFollowers); err != nil || followers != 3 {
		t.Fatalf("followers = %d err = %v", followers, err)
	}
	if len(paths) != 2 || paths[0] != "/users/me/people/friends" || paths[1] != "/users/me/people/followers" {
		t.Fatalf("paths = %v", paths)
	}
}
