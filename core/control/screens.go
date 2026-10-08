package control

import (
	"context"
	"encoding/json"
	"errors"

	"github.com/hashimthearab/rust-mcbe/core/catalog"
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
	FeaturedServers(ctx context.Context) ([]catalog.FeaturedServer, error)
	Profile(ctx context.Context) (catalog.Profile, error)
	// Ping needs no account; unreachable servers come back offline.
	Ping(ctx context.Context, addresses []string) []catalog.PingResult
	// Home gathers messaging, inbox, treatments, invites, live events and the persona head.
	Home(ctx context.Context) (catalog.Home, error)
	ReportMessage(ctx context.Context, event catalog.MessageEvent) error
}

// ScreenCountServices adds live populations only when experience details request them.
type ScreenCountServices interface {
	FeaturedServersWithCounts(context.Context) ([]catalog.FeaturedServer, error)
}

type homeResultV1 struct {
	SchemaVersion uint32       `json:"schema_version"`
	Home          catalog.Home `json:"home"`
}

// messageEventTypes are the reports a launcher may send.
var messageEventTypes = map[string]bool{
	"Click": true, "Dismiss": true, "Delete": true, "Impression": true, "ControlImpression": true, "ReadAll": true, "DeleteAllRead": true,
}

type pingResultV1 struct {
	SchemaVersion uint32               `json:"schema_version"`
	Servers       []catalog.PingResult `json:"servers"`
}

type featuredServersResultV1 struct {
	SchemaVersion uint32                   `json:"schema_version"`
	Servers       []catalog.FeaturedServer `json:"servers"`
}

type profileResultV1 struct {
	SchemaVersion uint32          `json:"schema_version"`
	Profile       catalog.Profile `json:"profile"`
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
		if !decodeParams(raw, &params) || len(params.Addresses) > catalog.MaxPingTargets {
			return nil, errInvalidParams
		}
		servers := screens.Ping(ctx, params.Addresses)
		if servers == nil {
			servers = []catalog.PingResult{}
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
		err := screens.ReportMessage(ctx, catalog.MessageEvent{
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
		var servers []catalog.FeaturedServer
		var err error
		if counts, ok := screens.(ScreenCountServices); includeCounts && ok {
			servers, err = counts.FeaturedServersWithCounts(ctx)
		} else {
			servers, err = screens.FeaturedServers(ctx)
		}
		if servers == nil {
			servers = []catalog.FeaturedServer{}
		}
		return featuredServersResultV1{SchemaVersion: 1, Servers: servers}, err
	default:
		profile, err := screens.Profile(ctx)
		return profileResultV1{SchemaVersion: 1, Profile: profile}, err
	}
}
