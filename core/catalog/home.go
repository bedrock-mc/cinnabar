package catalog

import (
	"context"
	"fmt"
	"net/http"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/hashimthearab/rust-mcbe/core/internal/locale"
	"github.com/sandertv/gophertunnel/minecraft/service"
	"github.com/sandertv/gophertunnel/minecraft/service/persona"
	"github.com/sandertv/gophertunnel/minecraft/service/playermessaging"
)

// Home is what the start screen shows from services: messaging surfaces and
// the inbox, the token's treatments, the Realms invite count and the rendered
// persona head. A failed part is named in Errors.
type Home struct {
	Messages     []Message `json:"messages"`
	Inbox        Inbox     `json:"inbox"`
	Treatments   []string  `json:"treatments"`
	RealmInvites int       `json:"realm_invites"`
	PersonaHead  Image     `json:"persona_head"`
	Errors       []string  `json:"errors,omitempty"`
	failed       homePart
}

// homePart marks one independently fetched part of Home.
type homePart uint8

const (
	partInvites homePart = 1 << iota
	partTreatments
	partMessages
	partPersona
	partServices = partTreatments | partMessages | partPersona
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
func (s *MessagingSession) get(discovery *service.Discovery, tokens service.TokenSource) (*playermessaging.Client, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.client == nil {
		env := new(playermessaging.Environment)
		if err := discovery.Environment(env); err != nil {
			return nil, fmt.Errorf("resolve messaging service: %w", err)
		}
		env.Language = s.language
		if env.Language == "" {
			env.Language = locale.Default
		}
		base := http.DefaultClient.Transport
		if base == nil {
			base = http.DefaultTransport
		}
		env.HTTPClient = &http.Client{Transport: messageArtTransport{RoundTripper: base, art: &s.art}}
		s.client = env.New(tokens)
	}
	return s.client, nil
}

// HomeFeed gathers the start screen's service data; the persona head is
// written into artworkDir.
func HomeFeed(ctx context.Context, account *authcache.Account, session *MessagingSession, artworkDir string) (Home, error) {
	home := Home{Messages: []Message{}, Treatments: []string{}}
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
		fail(partMessages|partPersona, "Services", fmt.Errorf("discover services: %w", err))
		return home, nil
	}
	if err := messages(ctx, discovery, account, session, &home); err != nil {
		fail(partMessages, "Messaging", err)
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
	client, err := RealmsClient(ctx, account)
	if err != nil {
		return 0, err
	}
	return client.PendingInviteCount(ctx)
}
