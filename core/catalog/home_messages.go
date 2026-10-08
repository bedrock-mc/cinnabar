package catalog

import (
	"encoding/json"
	"strings"

	"github.com/sandertv/gophertunnel/minecraft/service/playermessaging"
)

// applyMessageItems retains the art and ribbon text carried by multi-item messages.
func applyMessageItems(message *Message, items []playermessaging.MessageItem) {
	for _, item := range items {
		if message.SubTitle == "" {
			message.SubTitle = item.SubTitle
		}
		if message.Banner == "" {
			_ = json.Unmarshal(item.SaleBanner, &message.Banner)
		}
		if validArtworkURL(item.Image.URL) {
			message.Images = append(message.Images, MessageImage{ID: item.Image.ID, Image: Image{URL: item.Image.URL}})
		}
		if item.Button.Text != "" || item.Button.Link != "" {
			message.Buttons = append(message.Buttons, MessageButton{ID: item.Button.ID, Text: item.Button.Text, Link: item.Button.Link, Action: strings.ToLower(item.Button.Action)})
		}
	}
}
