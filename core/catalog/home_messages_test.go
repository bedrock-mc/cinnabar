package catalog

import (
	"encoding/json"
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
