package catalog

import (
	"encoding/json"
	"testing"

	playfab "github.com/df-mc/go-playfab/v2/catalog"
)

func TestDiscoveryKeepsBothGroupsAndCompleteArtworkRoles(t *testing.T) {
	const fixture = `[
 {"ContentType":"3PP_V2.0","Title":{"NEUTRAL":"Creator"},"Description":{"NEUTRAL":"Full description"},
  "DisplayProperties":{"url":"play.example.test","port":19132,"experienceId":"a26af7a2-ed3b-473d-8d1c-a5b4733b85bd","newsTitle":"Update","news":"News body","availableGames":[{"title":"Build","description":"Protect a base","imageTag":"Base"}]},
  "Images":[{"Tag":"Thumbnail","Url":"https://cdn.example.test/thumb.png"},{"Tag":"Icon","Type":"Screenshot","Url":"https://cdn.example.test/icon.png"},{"Tag":"Banner","Url":"https://cdn.example.test/banner.png"},{"Tag":"Base","Url":"https://cdn.example.test/base.png"}]},
 {"ContentType":"3PP_V2.0","Title":{"en-US":"Ranked"},"DisplayProperties":{"experienceId":"b7f5596c-e811-49ec-b318-80ff3c435d1d","rank":35}},
 {"ContentType":"3PP_V2.0","Title":{"NEUTRAL":"Zero"},"DisplayProperties":{"experienceId":"81ac183c-1d09-44a2-b0b5-78abaf8c9877","rank":0}},
 {"ContentType":"3PP_V2.0","Title":{"NEUTRAL":"Duplicate"},"DisplayProperties":{"url":"play.example.test","port":19132}},
 {"ContentType":"3PP_V2.0","Title":{"NEUTRAL":"Broken"},"DisplayProperties":{"experienceId":"bad"}},
 {"ContentType":"Other","Title":{"NEUTRAL":"Unrelated"},"DisplayProperties":{"url":"other.test","port":19132}}
 ]`
	var items []playfab.Item
	if err := json.Unmarshal([]byte(fixture), &items); err != nil {
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
