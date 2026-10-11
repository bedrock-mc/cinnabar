package catalog

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"strings"
	"testing"
	"time"

	"github.com/df-mc/go-xsapi/v2/social"
	"github.com/df-mc/go-xsapi/v2/xal/xsts"
	"github.com/sandertv/gophertunnel/minecraft/service"
)

// The catalog file names thumbnails by HTTPS URL only; the client caches them.
func TestCatalogServerCarriesOnlyHTTPSImageURLs(t *testing.T) {
	data, err := json.Marshal(catalogServer(FeaturedServer{Name: "A", thumbnailURL: "https://cdn.test/a.png"}))
	if err != nil || !strings.Contains(string(data), `"image_url":"https://cdn.test/a.png"`) {
		t.Fatalf("catalog row = %s, %v", data, err)
	}
	for _, raw := range []string{"http://cdn.test/a.png", "https://user:pw@cdn.test/a.png", ""} {
		if got := catalogServer(FeaturedServer{thumbnailURL: raw}).ImageURL; got != "" {
			t.Fatalf("catalog row kept %q", got)
		}
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
