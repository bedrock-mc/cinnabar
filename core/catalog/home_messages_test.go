package catalog

import (
	"encoding/json"
	"reflect"
	"testing"

	"github.com/sandertv/gophertunnel/minecraft/service/playermessaging"
)

func TestHomeMessageItemsKeepRibbonAndArt(t *testing.T) {
	var wire playermessaging.Message
	if err := json.Unmarshal([]byte(`{"messageItemList":[{"saleBanner":"Add-ons!","subTitle":"Build more","image":{"id":"defaultBackground","url":"https://example.com/art.png"}}]}`), &wire); err != nil {
		t.Fatal(err)
	}
	var message Message
	applyMessageItems(&message, wire.Items)
	if message.Banner != "Add-ons!" || message.SubTitle != "Build more" || len(message.Images) != 1 || message.Images[0].ID != "defaultBackground" {
		t.Fatalf("item fields lost: %+v", message)
	}
}

func TestFlattenMessageArtwork(t *testing.T) {
	var session playermessaging.Session
	if err := json.Unmarshal([]byte(`{
		"messages":[{
			"id":"top","instanceId":"shared","surface":"PlayButton","template":"ButtonArt",
			"messageText":{"bannerText":"Message banner"},
			"colors":{"BannerTextColor":{"hexColor":{"r":10,"g":20,"b":30}}},
			"images":{"z":{"url":"https://example.test/z.png"},"invalid":{"url":"file:///private.png"}},
			"buttons":{"z":{"text":"Last","action":"External"}},
			"messageItemList":[{
				"saleBanner":"Item banner","subTitle":"Build more",
				"image":{"id":"a","url":"https://example.test/a.png"},
				"button":{"id":"a","text":"First","link":"/play","action":"Internal"}
			}]
		}],
		"inboxSummary":{"categories":[{"messages":[{
			"id":"inbox","instanceId":"shared","surface":"InboxMessage","template":"ButtonArt",
			"messageText":{"bannerText":"Inbox banner"},
			"colors":{"BannerTextColor":{"hexColor":{"r":255,"g":0,"b":128}}}
		}]}]}
	}`), &session); err != nil {
		t.Fatal(err)
	}
	messages, _ := flatten(&session)
	if len(messages) != 2 {
		t.Fatalf("messages = %+v", messages)
	}
	for i, want := range []struct {
		banner string
		color  [3]uint8
	}{{"Message banner", [3]uint8{10, 20, 30}}, {"Inbox banner", [3]uint8{255, 0, 128}}} {
		if messages[i].Banner != want.banner || messages[i].Colors["BannerTextColor"] != want.color {
			t.Errorf("message %d artwork = %+v", i, messages[i])
		}
	}
	if messages[0].SubTitle != "Build more" {
		t.Errorf("subtitle = %q", messages[0].SubTitle)
	}
	wantImages := []MessageImage{
		{ID: "a", Image: Image{URL: "https://example.test/a.png"}},
		{ID: "z", Image: Image{URL: "https://example.test/z.png"}},
	}
	if !reflect.DeepEqual(messages[0].Images, wantImages) {
		t.Errorf("images = %+v, want %+v", messages[0].Images, wantImages)
	}
	wantButtons := []MessageButton{
		{ID: "a", Text: "First", Link: "/play", Action: "internal"},
		{ID: "z", Text: "Last", Action: "external"},
	}
	if !reflect.DeepEqual(messages[0].Buttons, wantButtons) {
		t.Errorf("buttons = %+v, want %+v", messages[0].Buttons, wantButtons)
	}
}

func TestFlattenMessageItemBannerFallback(t *testing.T) {
	var session playermessaging.Session
	if err := json.Unmarshal([]byte(`{"messages":[{
		"id":"m","surface":"PlayButton","template":"ButtonArt",
		"messageItemList":[
			{"saleBanner":{"text":"Not plain text"}},
			{"saleBanner":null},
			{"saleBanner":""},
			{"saleBanner":"First banner"},
			{"saleBanner":"Later banner"}
		]
	}]}`), &session); err != nil {
		t.Fatal(err)
	}
	messages, _ := flatten(&session)
	if len(messages) != 1 || messages[0].Banner != "First banner" {
		t.Fatalf("messages = %+v", messages)
	}
}
