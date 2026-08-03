// Package catalog loads the account-backed launcher destinations exposed by
// Bedrock services. It intentionally mirrors the small, composable calls used
// by Lunar instead of importing Lunar's application layer into the core.
package catalog

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/df-mc/go-playfab/v2"
	"github.com/df-mc/go-xsapi/v2"
	"github.com/google/uuid"
	"github.com/sandertv/gophertunnel/minecraft/auth"
	"github.com/sandertv/gophertunnel/minecraft/p2p"
	"github.com/sandertv/gophertunnel/minecraft/realms"
	"github.com/sandertv/gophertunnel/minecraft/service"
	"github.com/sandertv/gophertunnel/minecraft/service/gatherings"
	"golang.org/x/oauth2"
)

// File is the small JSON contract consumed by the Rust launcher.
type File struct {
	Featured   []Server `json:"featured"`
	Gatherings []Server `json:"gatherings"`
	Realms     []Realm  `json:"realms"`
	Friends    []Friend `json:"friends"`
	Errors     []string `json:"errors,omitempty"`
}

type Server struct {
	Name    string `json:"name"`
	Address string `json:"address"`
	Caption string `json:"caption"`
}

type Realm struct {
	Name    string `json:"name"`
	State   string `json:"state"`
	Target  string `json:"target"`
	Address string `json:"address,omitempty"`
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
func Fetch(ctx context.Context, src oauth2.TokenSource) (File, error) {
	if src == nil {
		return File{}, errors.New("catalog authentication token source is nil")
	}
	result := File{
		Featured:   []Server{},
		Gatherings: []Server{},
		Realms:     []Realm{},
		Friends:    []Friend{},
	}

	if values, err := fetchRealms(ctx, src); err != nil {
		result.Errors = append(result.Errors, "Realms: "+err.Error())
	} else {
		result.Realms = values
	}

	xbl, err := newXSAPIClient(ctx, src)
	if err != nil {
		result.Errors = append(result.Errors, "Friends: "+err.Error())
		result.Errors = append(result.Errors, "Featured servers: "+err.Error())
		result.Errors = append(result.Errors, "Gatherings: "+err.Error())
		return result, nil
	}
	defer xbl.Close()

	if values, err := fetchFriends(ctx, xbl); err != nil {
		result.Errors = append(result.Errors, "Friends: "+err.Error())
	} else {
		result.Friends = values
	}

	discovery, err := service.Default(ctx)
	if err != nil {
		result.Errors = append(result.Errors, "Featured servers: discover services: "+err.Error())
		result.Errors = append(result.Errors, "Gatherings: discover services: "+err.Error())
		return result, nil
	}
	env := new(service.AuthorizationEnvironment)
	if err := discovery.Environment(env); err != nil {
		result.Errors = append(result.Errors, "Featured servers: resolve services: "+err.Error())
		result.Errors = append(result.Errors, "Gatherings: resolve services: "+err.Error())
		return result, nil
	}
	playFab, err := playfab.LoginWithXbox(ctx, env.PlayFabTitleID, xbl, playfab.ClientConfig{CreateAccount: true})
	if err != nil {
		result.Errors = append(result.Errors, "Featured servers: PlayFab login: "+err.Error())
		result.Errors = append(result.Errors, "Gatherings: PlayFab login: "+err.Error())
		return result, nil
	}
	defer playFab.Close()

	gatheringsClient := gatherings.NewClient(env.TokenSource(playFab, service.TokenConfig{}))
	if values, err := gatheringsClient.FeaturedServers(ctx); err != nil {
		result.Errors = append(result.Errors, "Featured servers: "+err.Error())
	} else {
		for _, server := range values {
			if server == nil || !server.Valid() {
				continue
			}
			result.Featured = append(result.Featured, Server{
				Name:    displayName(server.Item.Title.Neutral(), server.CreatorName, "Featured server"),
				Address: server.Address(),
				Caption: firstGameCaption(server.AvailableGames, "Featured server"),
			})
		}
	}
	if values, err := gatheringsClient.Experiences(ctx); err != nil {
		result.Errors = append(result.Errors, "Gatherings: "+err.Error())
	} else {
		for _, experience := range values {
			if experience == nil || !experience.Valid() {
				continue
			}
			joinContext, cancel := context.WithTimeout(ctx, 5*time.Second)
			address, joinErr := experience.Join(joinContext)
			cancel()
			if joinErr != nil || address == nil || address.String() == ":0" {
				continue
			}
			result.Gatherings = append(result.Gatherings, Server{
				Name:    displayName(experience.Item.Title.Neutral(), experience.CreatorName, "Gathering"),
				Address: address.String(),
				Caption: firstGameCaption(experience.AvailableGames, "Community gathering"),
			})
		}
	}
	return result, nil
}

// Write fetches the catalog and publishes it as one complete JSON file so the
// Rust process never observes a partially-written response.
func Write(ctx context.Context, path string, src oauth2.TokenSource) error {
	if strings.TrimSpace(path) == "" {
		return errors.New("catalog output path is empty")
	}
	fetchContext, cancel := context.WithTimeout(ctx, 30*time.Second)
	defer cancel()
	result, err := Fetch(fetchContext, src)
	if err != nil {
		return err
	}
	contents, err := json.Marshal(result)
	if err != nil {
		return fmt.Errorf("encode catalog: %w", err)
	}
	absolute, err := filepath.Abs(path)
	if err != nil {
		return fmt.Errorf("resolve catalog output: %w", err)
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

func fetchRealms(ctx context.Context, src oauth2.TokenSource) ([]Realm, error) {
	values, err := realms.NewClient(src, nil).Realms(ctx)
	if err != nil {
		return nil, err
	}
	result := make([]Realm, 0, len(values))
	for _, realm := range values {
		entry := Realm{
			Name:   displayName(realm.Name, "", "Realm"),
			State:  realm.State,
			Target: fmt.Sprintf("realm_id/%d", realm.ID),
		}
		joinContext, cancel := context.WithTimeout(ctx, 4*time.Second)
		address, addressErr := realm.Address(joinContext)
		cancel()
		if addressErr == nil {
			entry.Address = address.Address
		}
		result = append(result, entry)
	}
	return result, nil
}

func newXSAPIClient(ctx context.Context, src oauth2.TokenSource) (*xsapi.Client, error) {
	client, err := xsapi.ClientConfig{RTAMode: xsapi.RTALazy}.New(ctx, auth.AndroidConfig.New(src, nil))
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
		if world.OwnerID == "" || world.OwnerID == currentXUID || world.HostName == "" {
			continue
		}
		if world.RealmID != 0 || world.ExperienceID != uuid.Nil || world.ExperienceWorldID != uuid.Nil || world.FriendID != "" {
			continue
		}
		if world.Joinability != p2p.JoinabilityFriends {
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

func displayName(values ...string) string {
	for _, value := range values {
		if value = strings.TrimSpace(value); value != "" {
			return value
		}
	}
	return "Minecraft"
}

func firstGameCaption(values []gatherings.AvailableGame, fallback string) string {
	for _, value := range values {
		if title := strings.TrimSpace(value.Title); title != "" {
			return title
		}
		if subtitle := strings.TrimSpace(value.Subtitle); subtitle != "" {
			return subtitle
		}
	}
	return fallback
}
