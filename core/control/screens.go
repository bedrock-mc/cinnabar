package control

import (
	"context"
	"encoding/json"
	"errors"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
)

// Menu screen feeds served beside the account methods.
const (
	methodFeaturedServers = "featured_servers.v1"
	methodProfile         = "profile.v1"
	methodPing            = "ping.v1"
	methodHome            = "home.v1"
	methodMessageEvent    = "message_event.v1"
)

var errInvalidParams = errors.New("control: invalid params")

// ScreenServices feeds the start and play screens; a Services value may implement it.
type ScreenServices interface {
	FeaturedServers(ctx context.Context) ([]FeaturedServer, error)
	Profile(ctx context.Context) (Profile, error)
	// Ping needs no account; unreachable servers come back offline.
	Ping(ctx context.Context, addresses []string) []PingResult
	// Home gathers messaging, inbox, treatments, invites, live events and the persona head.
	Home(ctx context.Context) (Home, error)
	ReportMessage(ctx context.Context, event MessageEvent) error
}

// ScreenCountServices adds live populations only when experience details request them.
type ScreenCountServices interface {
	FeaturedServersWithCounts(context.Context) ([]FeaturedServer, error)
}

// Image is remote HTTPS artwork, or a local path for art the core renders.
type Image struct {
	URL  string `json:"url,omitempty"`
	Path string `json:"path,omitempty"`
}

// FeaturedServer is a featured server with the details the play screen's info panel shows.
type FeaturedServer struct {
	Name        string   `json:"name"`
	Group       string   `json:"group"`
	PlayerCount *int64   `json:"player_count,omitempty"`
	Address     string   `json:"address"`
	Caption     string   `json:"caption"`
	Description string   `json:"description,omitempty"`
	NewsTitle   string   `json:"news_title,omitempty"`
	News        string   `json:"news,omitempty"`
	Logo        Image    `json:"logo"`
	Background  Image    `json:"background"` // the details panel's banner
	Screenshots []Image  `json:"screenshots"`
	Games       []Game   `json:"games"`
	Tags        []string `json:"tags,omitempty"`
}

// Game is one activity a featured server or gathering advertises.
type Game struct {
	Title       string `json:"title"`
	Subtitle    string `json:"subtitle,omitempty"`
	Description string `json:"description,omitempty"`
	Image       Image  `json:"image"`
}

// Profile is the signed-in account; a count whose lookup failed is omitted rather than zero.
type Profile struct {
	Gamertag                string               `json:"gamertag"`
	XUID                    string               `json:"xuid"`
	Gamerpic                Image                `json:"gamerpic"`
	Avatar                  Image                `json:"avatar"`
	AvatarError             bool                 `json:"avatar_error,omitempty"`
	FeaturedScreenshot      Image                `json:"featured_screenshot"`
	FeaturedScreenshotError bool                 `json:"featured_screenshot_error,omitempty"`
	RealName                string               `json:"real_name,omitempty"`
	PresenceText            string               `json:"presence_text,omitempty"`
	Gamerscore              *int64               `json:"gamerscore,omitempty"`
	Friends                 *int                 `json:"friends,omitempty"`
	Followers               *int                 `json:"followers,omitempty"`
	Statistics              *ProfileStatistics   `json:"statistics,omitempty"`
	Achievements            *ProfileAchievements `json:"achievements,omitempty"`
}

// ProfileStatistics are the title statistics the profile overview shows, at service precision.
type ProfileStatistics struct {
	MinutesPlayed     *string `json:"minutes_played,omitempty"`
	BlocksBroken      *string `json:"blocks_broken,omitempty"`
	MobsDefeated      *string `json:"mobs_defeated,omitempty"`
	DistanceTravelled *string `json:"distance_travelled,omitempty"`
}

// ProfileAchievements summarises the account's achievements for the profile overview.
type ProfileAchievements struct {
	Unlocked          int                  `json:"unlocked"`
	Total             int                  `json:"total"`
	CurrentGamerscore *int64               `json:"current_gamerscore"`
	MaxGamerscore     *int64               `json:"max_gamerscore"`
	Entries           []ProfileAchievement `json:"entries"`
}

// ProfileAchievement is one achievement as the Xbox service describes it.
type ProfileAchievement struct {
	ID             string `json:"id"`
	Name           string `json:"name"`
	Description    string `json:"description"`
	Image          Image  `json:"image"`
	Gamerscore     *int64 `json:"gamerscore,omitempty"`
	Locked         bool   `json:"locked"`
	DateUnlocked   string `json:"date_unlocked,omitempty"`
	SuggestedOrder *int   `json:"suggested_order,omitempty"`
}

// PingResult is one server's RakNet pong as the server rows show it.
type PingResult struct {
	Address    string `json:"address"`
	Online     bool   `json:"online"`
	Players    int    `json:"players"`
	MaxPlayers int    `json:"max_players"`
	PingMillis int64  `json:"ping_ms"`
	MOTD       string `json:"motd,omitempty"`
}

// Home is what the start screen shows from services; a failed part is named in Errors.
type Home struct {
	Messages     []Message `json:"messages"`
	Inbox        Inbox     `json:"inbox"`
	Treatments   []string  `json:"treatments"`
	RealmInvites int       `json:"realm_invites"`
	PersonaHead  Image     `json:"persona_head"`
	Errors       []string  `json:"errors,omitempty"`
}

// Message is one player-messaging message; Surface places it.
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

// MessageButton is one keyed message button; Action says how Link opens.
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

// InboxCategory is one inbox category's counts.
type InboxCategory struct {
	Type   string `json:"type"`
	Name   string `json:"name"`
	Total  int    `json:"total"`
	Unread int    `json:"unread"`
}

// MessageEvent is one messaging report.
type MessageEvent struct {
	Type       string
	InstanceID string
	ReportID   string
	ButtonID   string
}

type homeResultV1 struct {
	SchemaVersion uint32 `json:"schema_version"`
	Home          Home   `json:"home"`
}

// messageEventTypes are the reports a launcher may send.
var messageEventTypes = map[string]bool{
	"Click": true, "Dismiss": true, "Delete": true, "Impression": true, "ControlImpression": true, "ReadAll": true, "DeleteAllRead": true,
}

type pingResultV1 struct {
	SchemaVersion uint32       `json:"schema_version"`
	Servers       []PingResult `json:"servers"`
}

type featuredServersResultV1 struct {
	SchemaVersion uint32           `json:"schema_version"`
	Servers       []FeaturedServer `json:"servers"`
}

type profileResultV1 struct {
	SchemaVersion uint32  `json:"schema_version"`
	Profile       Profile `json:"profile"`
}

func isScreenMethod(method string) bool {
	switch method {
	case methodFeaturedServers, methodProfile, methodPing, methodHome, methodMessageEvent:
		return true
	}
	return false
}

func screenResult(ctx context.Context, screens ScreenServices, method string, raw json.RawMessage) (any, error) {
	if method == methodPing {
		var params struct {
			Addresses []string `json:"addresses"`
		}
		if !decodeParams(raw, &params) || len(params.Addresses) > bridgecontract.MaxPingTargets {
			return nil, errInvalidParams
		}
		servers := screens.Ping(ctx, params.Addresses)
		if servers == nil {
			servers = []PingResult{}
		}
		return pingResultV1{SchemaVersion: 1, Servers: servers}, nil
	}
	if method == methodMessageEvent {
		var params struct {
			Type       string `json:"event_type"`
			InstanceID string `json:"instance_id"`
			ReportID   string `json:"report_id"`
			ButtonID   string `json:"button_id"`
		}
		if !decodeParams(raw, &params) || !messageEventTypes[params.Type] {
			return nil, errInvalidParams
		}
		err := screens.ReportMessage(ctx, MessageEvent{
			Type: params.Type, InstanceID: params.InstanceID, ReportID: params.ReportID, ButtonID: params.ButtonID,
		})
		return emptyResultV1{SchemaVersion: 1}, err
	}
	includeCounts := false
	if method == methodFeaturedServers && len(raw) != 0 {
		var params struct {
			IncludePlayerCounts bool `json:"include_player_counts"`
		}
		if !decodeParams(raw, &params) {
			return nil, errInvalidParams
		}
		includeCounts = params.IncludePlayerCounts
	} else if len(raw) != 0 {
		return nil, errInvalidParams
	}
	switch method {
	case methodHome:
		home, err := screens.Home(ctx)
		return homeResultV1{SchemaVersion: 1, Home: home}, err
	case methodFeaturedServers:
		var servers []FeaturedServer
		var err error
		if counts, ok := screens.(ScreenCountServices); includeCounts && ok {
			servers, err = counts.FeaturedServersWithCounts(ctx)
		} else {
			servers, err = screens.FeaturedServers(ctx)
		}
		if servers == nil {
			servers = []FeaturedServer{}
		}
		return featuredServersResultV1{SchemaVersion: 1, Servers: servers}, err
	default:
		profile, err := screens.Profile(ctx)
		return profileResultV1{SchemaVersion: 1, Profile: profile}, err
	}
}
