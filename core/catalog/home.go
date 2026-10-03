package catalog

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/hashimthearab/rust-mcbe/core/clientplatform"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/realms"
	"github.com/sandertv/gophertunnel/minecraft/service"
	"github.com/sandertv/gophertunnel/minecraft/service/gatherings"
	"github.com/sandertv/gophertunnel/minecraft/service/persona"
	"github.com/sandertv/gophertunnel/minecraft/service/playermessaging"
)

// Home is what the start screen shows from services: messaging surfaces and
// the inbox, the token's treatments, the Realms invite count, live events and
// the rendered persona head. A failed part is named in Errors.
type Home struct {
	Messages     []Message   `json:"messages"`
	Inbox        Inbox       `json:"inbox"`
	Treatments   []string    `json:"treatments"`
	RealmInvites int         `json:"realm_invites"`
	LiveEvents   []LiveEvent `json:"live_events"`
	PersonaHead  Image       `json:"persona_head"`
	Errors       []string    `json:"errors,omitempty"`
	failed       homePart
}

// homePart marks one independently fetched part of Home.
type homePart uint8

const (
	partInvites homePart = 1 << iota
	partTreatments
	partMessages
	partEvents
	partPersona
	partServices = partTreatments | partMessages | partEvents | partPersona
	allHomeParts = partInvites | partServices
)

// Failed reports whether no part of a fetched Home succeeded.
func (h Home) Failed() bool { return h.failed == allHomeParts }

// Refill returns h with its failed parts taken from previous, so a partial refresh keeps the
// last good data.
func (h Home) Refill(previous Home) Home {
	if h.failed&partInvites != 0 {
		h.RealmInvites = previous.RealmInvites
	}
	if h.failed&partTreatments != 0 {
		h.Treatments = previous.Treatments
	}
	if h.failed&partMessages != 0 {
		h.Messages, h.Inbox = previous.Messages, previous.Inbox
	}
	if h.failed&partEvents != 0 {
		h.LiveEvents = previous.LiveEvents
	}
	if h.failed&partPersona != 0 {
		h.PersonaHead = previous.PersonaHead
	}
	return h
}

// HomeImages lists the artwork of home for CacheImages.
func HomeImages(home *Home) []*Image {
	var images []*Image
	for index := range home.Messages {
		for image := range home.Messages[index].Images {
			images = append(images, &home.Messages[index].Images[image].Image)
		}
	}
	for index := range home.LiveEvents {
		images = append(images, &home.LiveEvents[index].Badge, &home.LiveEvents[index].EventImage)
	}
	return append(images, &home.PersonaHead)
}

// Message is one player-messaging message; Surface places it (PlayButton,
// MarketplaceButton, InboxMessage, LoginAnnouncement, ToastNotification, ...).
type Message struct {
	Colors     map[string][3]uint8 `json:"colors,omitempty"`
	Sender     string              `json:"sender,omitempty"`
	ID         string              `json:"id"`
	InstanceID string              `json:"instance_id"`
	ReportID   string              `json:"report_id,omitempty"`
	Surface    string              `json:"surface"`
	Template   string              `json:"template"`
	Category   string              `json:"category,omitempty"`
	Status     string              `json:"status,omitempty"`
	Received   string              `json:"received,omitempty"`
	Header     string              `json:"header,omitempty"`
	Body       string              `json:"body,omitempty"`
	SubTitle   string              `json:"sub_title,omitempty"`
	Banner     string              `json:"banner,omitempty"`
	Images     []MessageImage      `json:"images"`
	Buttons    []MessageButton     `json:"buttons"`
}

// MessageImage is one keyed message image.
type MessageImage struct {
	ID string `json:"id"`
	Image
}

// MessageButton is one keyed message button; Action is external, internal,
// pageid or productid and says how Link opens.
type MessageButton struct {
	ID     string `json:"id"`
	Text   string `json:"text"`
	Link   string `json:"link,omitempty"`
	Action string `json:"action,omitempty"`
}

// Inbox is the inbox summary with per-category counts.
type Inbox struct {
	Total      int             `json:"total"`
	Unread     int             `json:"unread"`
	Categories []InboxCategory `json:"categories"`
}

type InboxCategory struct {
	Type   string `json:"type"`
	Name   string `json:"name"`
	Total  int    `json:"total"`
	Unread int    `json:"unread"`
}

// LiveEvent is a live gathering with the active segment's start-screen UI.
type LiveEvent struct {
	ID                string `json:"id"`
	Title             string `json:"title"`
	Description       string `json:"description,omitempty"`
	StartUnix         int64  `json:"start_unix"`
	EndUnix           int64  `json:"end_unix"`
	RouteToServers    bool   `json:"route_to_servers,omitempty"`
	Address           string `json:"address,omitempty"`
	NetherNetID       string `json:"nethernet_id,omitempty"`
	ButtonText        string `json:"button_text,omitempty"`
	CaptionText       string `json:"caption_text,omitempty"`
	CaptionCountdown  bool   `json:"caption_countdown,omitempty"`
	CaptionBackground string `json:"caption_background,omitempty"`
	CaptionForeground string `json:"caption_foreground,omitempty"`
	HeaderText        string `json:"header_text,omitempty"`
	TitleText         string `json:"title_text,omitempty"`
	BodyText          string `json:"body_text,omitempty"`
	Badge             Image  `json:"badge"`
	EventImage        Image  `json:"event_image"`
}

// MessagingSession holds the account's messaging session across home refreshes and reports.
type MessagingSession struct {
	art      messageArt
	mu       sync.Mutex
	client   *playermessaging.Client
	language string
}

// NewMessagingSession keeps the active UI language for refreshes and reports.
func NewMessagingSession(language string) *MessagingSession {
	return &MessagingSession{language: language}
}

// get returns the session's client, opening it on the discovered endpoint on first use.
func (s *MessagingSession) get(discovery *service.Discovery, account *authcache.Account) (*playermessaging.Client, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.client == nil {
		env := new(playermessaging.Environment)
		if err := discovery.Environment(env); err != nil {
			return nil, fmt.Errorf("resolve messaging service: %w", err)
		}
		env.HTTPClient = messagingHTTPClient(env.HTTPClient, s.language)
		env.HTTPClient.Transport = messageArtTransport{RoundTripper: env.HTTPClient.Transport, art: &s.art}
		s.client = env.New(account)
	}
	return s.client, nil
}

// HomeFeed gathers the start screen's service data; the persona head is
// written into artworkDir.
func HomeFeed(ctx context.Context, account *authcache.Account, session *MessagingSession, artworkDir string) (Home, error) {
	home := Home{Messages: []Message{}, Treatments: []string{}, LiveEvents: []LiveEvent{}}
	if account == nil {
		return home, errNoAccount
	}
	fail := func(part homePart, name string, err error) {
		home.failed |= part
		home.Errors = append(home.Errors, name+": "+err.Error())
	}
	if count, err := realmInvites(ctx, account); err != nil {
		fail(partInvites, "Realms invites", err)
	} else {
		home.RealmInvites = count
	}
	if token, err := account.ServiceToken(ctx); err == nil {
		home.Treatments = append(home.Treatments, token.Treatments...)
	} else {
		fail(partTreatments, "Treatments", err)
	}
	discovery, err := service.Default(ctx)
	if err != nil {
		fail(partMessages|partEvents|partPersona, "Services", fmt.Errorf("discover services: %w", err))
		return home, nil
	}
	if err := messages(ctx, discovery, account, session, &home); err != nil {
		fail(partMessages, "Messaging", err)
	}
	if events, err := liveEvents(ctx, discovery, account, time.Now()); err != nil {
		fail(partEvents, "Live events", err)
	} else {
		home.LiveEvents = events
	}
	if head, err := personaHead(ctx, discovery, account, artworkDir); err != nil {
		fail(partPersona, "Persona", err)
	} else {
		home.PersonaHead = head
	}
	return home, nil
}

// ReportMessageEvent posts one messaging event (Impression, Click, Dismiss, ...).
func ReportMessageEvent(ctx context.Context, account *authcache.Account, session *MessagingSession, event MessageEvent) error {
	if account == nil {
		return errNoAccount
	}
	discovery, err := service.Default(ctx)
	if err != nil {
		return fmt.Errorf("discover services: %w", err)
	}
	client, err := session.get(discovery, account)
	if err != nil {
		return err
	}
	return client.ReportEvents(ctx, playermessaging.Event{
		Type: playermessaging.EventType(event.Type), InstanceID: event.InstanceID, ReportID: event.ReportID, ButtonID: event.ButtonID,
	})
}

// MessageEvent is one messaging report.
type MessageEvent struct {
	Type       string
	InstanceID string
	ReportID   string
	ButtonID   string
}

func messages(ctx context.Context, discovery *service.Discovery, account *authcache.Account, session *MessagingSession, home *Home) error {
	client, err := session.get(discovery, account)
	if err != nil {
		return err
	}
	refreshed, err := client.Refresh(ctx)
	if err != nil {
		return err
	}
	home.Messages, home.Inbox = flatten(refreshed)
	session.art.apply(home.Messages)
	return nil
}

// flatten keeps well-formed messages (id, surface and template set), top-level
// then per-category, each (id, instance) once, plus the inbox counts.
func flatten(session *playermessaging.Session) ([]Message, Inbox) {
	inbox := Inbox{Total: session.InboxSummary.Total, Categories: []InboxCategory{}}
	all := append([]playermessaging.Message(nil), session.Messages...)
	for _, category := range session.InboxSummary.Categories {
		inbox.Unread += max(category.Unread, 0)
		inbox.Categories = append(inbox.Categories, InboxCategory{
			Type: category.Info.Type, Name: category.Info.Name, Total: category.Total, Unread: category.Unread,
		})
		all = append(all, category.Messages...)
	}
	seen := make(map[[2]string]bool)
	messages := []Message{}
	for _, wire := range all {
		key := [2]string{wire.ID, wire.InstanceID}
		if wire.ID == "" || wire.Surface == "" || wire.Template == "" || seen[key] {
			continue
		}
		seen[key] = true
		message := Message{
			ID: wire.ID, InstanceID: wire.InstanceID, ReportID: wire.ReportID,
			Surface: wire.Surface, Template: wire.Template, Category: wire.InboxCategory,
			Status: wire.Status, Received: wire.DateReceived, Sender: wire.Sender,
			Header: wire.Text.Header, Body: wire.Text.Body,
			Images: []MessageImage{}, Buttons: []MessageButton{},
		}
		for id, image := range wire.Images {
			if validArtworkURL(image.URL) {
				message.Images = append(message.Images, MessageImage{ID: id, Image: Image{URL: image.URL}})
			}
		}
		for id, button := range wire.Buttons {
			message.Buttons = append(message.Buttons, MessageButton{
				ID: id, Text: button.Text, Link: button.Link, Action: strings.ToLower(button.Action),
			})
		}
		applyMessageItems(&message, wire.Items)
		slices.SortFunc(message.Images, func(a, b MessageImage) int { return strings.Compare(a.ID, b.ID) })
		slices.SortFunc(message.Buttons, func(a, b MessageButton) int { return strings.Compare(a.ID, b.ID) })
		messages = append(messages, message)
	}
	return messages, inbox
}

// liveEvents fetches the desktop public configuration from the discovered service.
func liveEvents(ctx context.Context, discovery *service.Discovery, account service.TokenSource, now time.Time) ([]LiveEvent, error) {
	client, err := gatheringsClient(discovery, account)
	if err != nil {
		return nil, err
	}
	configs, err := client.PublicConfig(ctx, gatherings.ConfigQuery{
		ClientVersion: protocol.CurrentVersion, ClientPlatform: clientplatform.Platform, ClientSubPlatform: clientplatform.SubPlatform,
	})
	if err != nil {
		return nil, err
	}
	return liveEventsFrom(configs, now), nil
}

// liveEventsFrom keeps the events not yet over, each dressed by the segment running now.
func liveEventsFrom(configs []gatherings.GatheringConfig, now time.Time) []LiveEvent {
	events := []LiveEvent{}
	for _, config := range configs {
		if config.ID == "" || (!config.End.IsZero() && config.End.Before(now)) {
			continue
		}
		event := LiveEvent{
			ID: config.ID, Title: strings.TrimSpace(config.Title), Description: strings.TrimSpace(config.Description),
			StartUnix: unixOf(config.Start), EndUnix: unixOf(config.End), RouteToServers: config.RouteToServersTab,
			NetherNetID: config.Venue.NetherNetID,
		}
		if address, ok := config.Venue.RakNetAddress(); ok {
			event.Address = address
		}
		// The segment running now dresses the button; else the first one.
		for index, segment := range config.Segments {
			running := !segment.Start.After(now) && (segment.End.IsZero() || now.Before(segment.End.Time))
			if index == 0 || running {
				applySegmentUI(&event, segment.UI)
			}
		}
		events = append(events, event)
	}
	return events
}

func applySegmentUI(event *LiveEvent, ui gatherings.SegmentUI) {
	event.ButtonText, event.CaptionText = ui.StartScreenButtonText, ui.CaptionText
	event.CaptionCountdown = ui.CaptionCountdown
	event.CaptionBackground, event.CaptionForeground = ui.CaptionBackgroundColor, ui.CaptionForegroundColor
	event.HeaderText, event.TitleText, event.BodyText = ui.HeaderText, ui.TitleText, ui.BodyText
	event.Badge, event.EventImage = Image{}, Image{}
	if validArtworkURL(ui.BadgeImage) {
		event.Badge.URL = ui.BadgeImage
	}
	if validArtworkURL(ui.EventImage) {
		event.EventImage.URL = ui.EventImage
	}
}

func unixOf(t gatherings.Time) int64 {
	if t.IsZero() {
		return 0
	}
	return t.Unix()
}

// personaHead writes the rendered persona head for the signed-in account into artworkDir.
func personaHead(ctx context.Context, discovery *service.Discovery, account *authcache.Account, artworkDir string) (Image, error) {
	env := new(persona.Environment)
	if err := discovery.Environment(env); err != nil {
		return Image{}, fmt.Errorf("resolve persona service: %w", err)
	}
	xbl, err := XboxClient(ctx, account)
	if err != nil {
		return Image{}, err
	}
	xuid := xbl.UserInfo().XUID
	_ = xbl.Close()
	head, err := env.New(account).ProfileImage(ctx, xuid, persona.ImageHead)
	if err != nil || artworkDir == "" {
		return Image{}, err
	}
	if err := os.MkdirAll(artworkDir, 0o700); err != nil {
		return Image{}, err
	}
	path := filepath.Join(artworkDir, "persona-head.img")
	if err := os.WriteFile(path, head.Data, 0o600); err != nil {
		return Image{}, err
	}
	return Image{Path: path}, nil
}

// realmInvites reads the pending Realms invite count through the Realms client.
func realmInvites(ctx context.Context, account *authcache.Account) (int, error) {
	return realms.NewClient(account, nil).PendingInviteCount(ctx)
}
