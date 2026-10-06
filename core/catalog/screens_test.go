package catalog

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/df-mc/go-xsapi/v2/social"
	"github.com/df-mc/go-xsapi/v2/xal/xsts"
	"github.com/hashimthearab/rust-mcbe/core/internal/imagecache"
	"github.com/sandertv/gophertunnel/minecraft/service"
	"github.com/sandertv/gophertunnel/minecraft/service/layout"
)

// Synthesized in the live ServerTab shape: a featured experience with its details, then a list
// repeating it beside a listing-only experience. Not a captured payload.
const serverTabFixture = `{"title":{"value":""},"refreshPolicy":{"timeToLiveInSeconds":3600},"body":{"fabs":[
{"$type":"ExperienceFab","id":"hero","variant":"feature-play","experience":{"experienceId":"b7f5596c-e811-49ec-b318-80ff3c435d1d",
 "title":{"value":" OneBlock "},"creatorName":"Maker","description":{"value":" Skyblock. "},
 "backgroundImage":{"full":{"url":"https://cdn.example.test/bg.png"}},
 "logoImage":{"full":{"url":"https://cdn.example.test/logo.png"}},
 "activities":[{"title":{"value":"Play"},"subtitle":{"value":"Solo"},"description":{"value":"Go"},"image":{"half":{"url":"https://cdn.example.test/a.png"}}},
  {"title":{"value":""},"subtitle":{"value":""}},{"title":{"value":"Insecure"},"image":{"full":{"url":"http://insecure.test/x.png"}}}],
 "listing":{"displayImage":{"full":{"url":"https://cdn.example.test/f.png"}},"motd":{"value":"Hello"}}}},
{"$type":"ExperienceListFab","id":"all","variant":"grid","pagedExperiences":{"experiences":[
 {"experienceId":"b7f5596c-e811-49ec-b318-80ff3c435d1d","title":{"value":"OneBlock"}},
 {"experienceId":"81ac183c-1d09-44a2-b0b5-78abaf8c9877","title":{"value":""},"creatorName":"Hunter",
  "listing":{"displayImage":{"quarter":{"url":"https://cdn.example.test/q.png"}},"motd":{"value":""}}}]}}]}}`

// Every Servers tab experience is listed once, joined by its id at connect time, with the info
// panel details its featured fab carries.
func TestServerTabExperiencesFeedTheFeaturedList(t *testing.T) {
	var tab layout.Layout
	if err := json.Unmarshal([]byte(serverTabFixture), &tab); err != nil {
		t.Fatal(err)
	}
	servers := featuredServers(&tab)
	if len(servers) != 2 {
		t.Fatalf("servers = %+v", servers)
	}
	hero, listed := servers[0], servers[1]
	if hero.Name != "OneBlock" || hero.Address != GatheringTargetPrefix+"b7f5596c-e811-49ec-b318-80ff3c435d1d" ||
		hero.Caption != "Hello" || hero.Description != "Skyblock." || hero.Logo.URL != "https://cdn.example.test/logo.png" ||
		hero.thumbnailURL != "https://cdn.example.test/f.png" || hero.Background.URL != "https://cdn.example.test/bg.png" {
		t.Fatalf("hero = %+v", hero)
	}
	if len(hero.Games) != 2 || hero.Games[0].Image.URL != "https://cdn.example.test/a.png" || hero.Games[1].Image.URL != "" {
		t.Fatalf("games keep only titled activities and HTTPS art: %+v", hero.Games)
	}
	if listed.Name != "Hunter" || listed.Caption != "Featured server" || listed.thumbnailURL != "https://cdn.example.test/q.png" ||
		listed.Logo.URL != "https://cdn.example.test/q.png" {
		t.Fatalf("listed = %+v", listed)
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
	if len(images) != 4 {
		t.Fatalf("images = %d", len(images))
	}
	images[1].Path, images[2].Path = "/cache/bg.img", "/cache/s.img"
	if servers[0].Background.Path != "/cache/bg.img" || servers[0].Screenshots[0].Path != "/cache/s.img" {
		t.Fatal("paths must land in the servers")
	}
}

type fixedTokens struct{}

func (fixedTokens) ServiceToken(context.Context) (*service.Token, error) {
	return &service.Token{AuthorizationHeader: "MCToken synthetic", ValidUntil: time.Now().Add(time.Hour)}, nil
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
