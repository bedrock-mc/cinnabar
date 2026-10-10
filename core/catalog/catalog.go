// Package catalog loads the account-backed launcher destinations exposed by
// Bedrock services. It intentionally mirrors the small, composable calls used
// by Lunar instead of importing Lunar's application layer into the core.
package catalog

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/df-mc/go-xsapi/v2"
	"github.com/google/uuid"
	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/sandertv/gophertunnel/minecraft/p2p"
	"github.com/sandertv/gophertunnel/minecraft/realms"
	"github.com/sandertv/gophertunnel/minecraft/service"
	"golang.org/x/oauth2"
	"net/http"
	"sync"
)

// File is the small JSON contract consumed by the Rust launcher.
type File struct {
	Featured []Server `json:"featured"`
	Realms   []Realm  `json:"realms"`
	Friends  []Friend `json:"friends"`
	Errors   []string `json:"errors,omitempty"`
}

type Server struct {
	Name     string `json:"name"`
	Address  string `json:"address"`
	Caption  string `json:"caption"`
	ImageURL string `json:"image_url,omitempty"`
}

type Realm struct {
	Name   string `json:"name"`
	State  string `json:"state"`
	Target string `json:"target"`
	// Details the realms grid binds; all optional.
	Owner         string `json:"owner,omitempty"`
	MOTD          string `json:"motd,omitempty"`
	WorldType     string `json:"world_type,omitempty"`
	OnlinePlayers int    `json:"online_players"`
	MaxPlayers    int    `json:"max_players,omitempty"`
	DaysLeft      int    `json:"days_left,omitempty"`
	Expired       bool   `json:"expired,omitempty"`
	Member        bool   `json:"member,omitempty"` // joined, not owned
}

type Friend struct {
	Gamertag   string `json:"gamertag"`
	XUID       string `json:"xuid"`
	WorldName  string `json:"world_name"`
	Members    int    `json:"members"`
	MaxMembers int    `json:"max_members"`
	HandleID   string `json:"handle_id,omitempty"`
	Address    string `json:"address,omitempty"`
}

// Fetch loads the four account-backed launcher surfaces. A failure in one
// service is retained in Errors so the UI can show the successful sections and
// explain a missing section instead of silently presenting fake servers.
func Fetch(ctx context.Context, account *authcache.Account) (File, error) {
	if account == nil {
		return File{}, errNoAccount
	}
	result := File{
		Featured: []Server{},
		Realms:   []Realm{},
		Friends:  []Friend{},
	}

	if values, err := Realms(ctx, account); err != nil {
		result.Errors = append(result.Errors, "Realms: "+err.Error())
	} else {
		result.Realms = values
	}

	if values, err := Friends(ctx, account); err != nil {
		result.Errors = append(result.Errors, "Friends: "+err.Error())
	} else {
		result.Friends = values
	}

	if values, err := FeaturedServers(ctx, account); err != nil {
		result.Errors = append(result.Errors, "Featured servers: "+err.Error())
	} else {
		for _, server := range values {
			result.Featured = append(result.Featured, catalogServer(server))
		}
	}
	return result, nil
}

// Write fetches the catalog and publishes it as one complete JSON file so the
// Rust process never observes a partially-written response.
func Write(ctx context.Context, path string, account *authcache.Account) error {
	if strings.TrimSpace(path) == "" {
		return errors.New("catalog output path is empty")
	}
	absolute, err := filepath.Abs(path)
	if err != nil {
		return fmt.Errorf("resolve catalog output: %w", err)
	}
	fetchContext, cancel := context.WithTimeout(ctx, 60*time.Second)
	defer cancel()
	result, err := Fetch(fetchContext, account)
	if err != nil {
		return err
	}
	contents, err := json.Marshal(result)
	if err != nil {
		return fmt.Errorf("encode catalog: %w", err)
	}
	if err := os.MkdirAll(filepath.Dir(absolute), 0o700); err != nil {
		return fmt.Errorf("create catalog output directory: %w", err)
	}
	temporary, err := os.CreateTemp(filepath.Dir(absolute), ".catalog-*.json")
	if err != nil {
		return fmt.Errorf("create catalog output: %w", err)
	}
	temporaryName := temporary.Name()
	defer os.Remove(temporaryName)
	if err := temporary.Chmod(0o600); err != nil {
		_ = temporary.Close()
		return fmt.Errorf("secure catalog output: %w", err)
	}
	if _, err := temporary.Write(contents); err != nil {
		_ = temporary.Close()
		return fmt.Errorf("write catalog output: %w", err)
	}
	if err := temporary.Close(); err != nil {
		return fmt.Errorf("close catalog output: %w", err)
	}
	if err := os.Rename(temporaryName, absolute); err != nil {
		return fmt.Errorf("publish catalog output: %w", err)
	}
	return nil
}

// catalogServer is the compact catalog row; the client downloads its thumbnail URL.
func catalogServer(server FeaturedServer) Server {
	entry := Server{Name: server.Name, Address: server.Address, Caption: server.Caption}
	if validArtworkURL(server.thumbnailURL) {
		entry.ImageURL = server.thumbnailURL
	}
	return entry
}

// validArtworkURL accepts HTTPS image URLs without embedded credentials.
func validArtworkURL(raw string) bool {
	u, err := url.Parse(raw)
	return err == nil && u.Scheme == "https" && u.Hostname() != "" && u.User == nil
}

// Realms lists the Realms; the account supplies the Realms XSTS token from its shared cache.
func Realms(ctx context.Context, account *authcache.Account) ([]Realm, error) {
	if account == nil {
		return nil, errNoAccount
	}
	client, err := RealmsClient(ctx, account)
	if err != nil {
		return nil, err
	}
	return listRealms(ctx, client)
}

// RealmsClient returns the account's Realms client on the discovered endpoint, built once so its
// client-version negotiation and token cache last across calls.
func RealmsClient(ctx context.Context, account *authcache.Account) (*realms.Client, error) {
	if account == nil {
		return nil, errNoAccount
	}
	discovery, err := service.Default(ctx)
	if err != nil {
		return nil, fmt.Errorf("discover services: %w", err)
	}
	return sharedRealms.get(discovery, account, nil)
}

var sharedRealms realmsClients

// realmsClients keeps the Realms client of the account the core last served; a core serves one
// account at a time.
type realmsClients struct {
	mu      sync.Mutex
	account oauth2.TokenSource
	client  *realms.Client
}

func (c *realmsClients) get(discovery *service.Discovery, account oauth2.TokenSource, httpClient *http.Client) (*realms.Client, error) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.client != nil && c.account == account {
		return c.client, nil
	}
	env := new(realms.Environment)
	if err := discovery.Environment(env); err != nil {
		return nil, fmt.Errorf("resolve Realms service: %w", err)
	}
	client, err := env.NewClient(account, httpClient)
	if err != nil {
		return nil, fmt.Errorf("resolve Realms service: %w", err)
	}
	c.account, c.client = account, client
	return client, nil
}

// listRealms maps the client's Realms to catalog entries.
func listRealms(ctx context.Context, client *realms.Client) ([]Realm, error) {
	values, err := client.Realms(ctx)
	if err != nil {
		return nil, err
	}
	result := make([]Realm, 0, len(values))
	for _, realm := range values {
		entry := realmCard(realm)
		result = append(result, entry)
	}
	return result, nil
}

// GatheringTargetPrefix marks a catalog address as an experience ID joined at connect time.
const GatheringTargetPrefix = "gathering/"

// errNoAccount is returned when a call needs the signed-in account and there is none.
var errNoAccount = errors.New("catalog: no signed-in account")

// XboxClient signs in to Xbox Live with the account; the caller closes it.
func XboxClient(ctx context.Context, account *authcache.Account) (*xsapi.Client, error) {
	if account == nil {
		return nil, errNoAccount
	}
	client, err := xsapi.ClientConfig{RTAMode: xsapi.RTALazy}.New(ctx, account)
	if err != nil {
		return nil, fmt.Errorf("login to Xbox Live: %w", err)
	}
	return client, nil
}

func fetchFriends(ctx context.Context, client *xsapi.Client) ([]Friend, error) {
	worlds, err := p2p.NewClient(client).Worlds(ctx)
	if err != nil {
		return nil, fmt.Errorf("search friend worlds: %w", err)
	}
	currentXUID := client.UserInfo().XUID
	result := make([]Friend, 0, len(worlds))
	for _, world := range worlds {
		if !FriendWorldListed(world, currentXUID) {
			continue
		}
		connection, err := world.Connection()
		if err != nil {
			continue
		}
		handleID := ""
		if id := world.HandleID(); id != uuid.Nil {
			handleID = id.String()
		}
		result = append(result, Friend{
			Gamertag:   world.HostName,
			XUID:       world.OwnerID,
			WorldName:  world.WorldName,
			Members:    world.MemberCount,
			MaxMembers: world.MaxMemberCount,
			HandleID:   handleID,
			Address:    connection.Address(),
		})
	}
	return result, nil
}

// FriendWorldListed reports whether the friends tab lists world for the player self, by the game's
// rule. The activity query returns only sessions of people the player follows, so every host counts
// as a friend. Friends' Realm and experience sessions are left out: joining them is not implemented.
func FriendWorldListed(world p2p.World, self string) bool {
	if world.OwnerID == "" || world.RealmID != 0 || world.ExperienceWorldID != uuid.Nil || world.FriendID != "" {
		return false
	}
	return world.Listed(self, false, func(string) bool { return true })
}

func displayName(values ...string) string {
	for _, value := range values {
		if value = strings.TrimSpace(value); value != "" {
			return value
		}
	}
	return "Minecraft"
}
