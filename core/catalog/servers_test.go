package catalog

import (
	"encoding/json"
	"os"
	"testing"

	playfab "github.com/df-mc/go-playfab/v2/catalog"
)

func TestDiscoveryKeepsBothGroupsAndCompleteArtworkRoles(t *testing.T) {
	fixture, err := os.ReadFile("testdata/featured_projection.json")
	if err != nil {
		t.Fatal(err)
	}
	var items []playfab.Item
	if err := json.Unmarshal(fixture, &items); err != nil {
		t.Fatal(err)
	}
	servers := discoveryServers(items)
	if len(servers) != 3 {
		t.Fatalf("servers = %+v", servers)
	}
	if servers[0].Name != "Ranked" || servers[1].Name != "Zero" || servers[1].Group != "featured" || servers[2].Group != "creator" {
		t.Fatalf("groups and ranks = %+v", servers)
	}
	creator := servers[2]
	if creator.Address != "play.example.test:19132" || creator.Caption != "" || creator.Description != "Full description" || creator.News != "News body" {
		t.Fatalf("creator = %+v", creator)
	}
	if creator.Logo.URL != "https://cdn.example.test/icon.png" || creator.Background.URL != "https://cdn.example.test/banner.png" || creator.Games[0].Image.URL != "https://cdn.example.test/base.png" {
		t.Fatalf("art roles = %+v", creator)
	}
	if servers[0].Address != GatheringTargetPrefix+"b7f5596c-e811-49ec-b318-80ff3c435d1d" {
		t.Fatalf("experience target = %s", servers[0].Address)
	}
}

func TestServerArtworkFallsBackOnlyWithinItsRole(t *testing.T) {
	images := []playfab.Image{{Tag: "Icon", URL: "http://insecure.test/icon"}, {Tag: "Thumbnail", URL: "https://cdn.example.test/thumb"}}
	if got := serverImage(images, "Icon", "Thumbnail"); got.URL != images[1].URL {
		t.Fatalf("logo = %+v", got)
	}
	if got := serverImage(images, "Banner"); got.URL != "" {
		t.Fatalf("a thumbnail became a banner: %+v", got)
	}
}
