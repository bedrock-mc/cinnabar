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

	playfabcatalog "github.com/df-mc/go-playfab/v2/catalog"
	"github.com/df-mc/go-xsapi/v2"
	"github.com/google/uuid"
	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/hashimthearab/rust-mcbe/core/internal/imagecache"
	"github.com/sandertv/gophertunnel/minecraft/p2p"
	"github.com/sandertv/gophertunnel/minecraft/realms"
	"github.com/sandertv/gophertunnel/minecraft/service/gatherings"
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
	Name      string `json:"name"`
	Address   string `json:"address"`
	Caption   string `json:"caption"`
	ImagePath string `json:"image_path,omitempty"`
	imageURL  string
}

type Realm struct {
	Name    string `json:"name"`
	State   string `json:"state"`
	Target  string `json:"target"`
	Address string `json:"address,omitempty"`
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
		Featured:   []Server{},
		Gatherings: []Server{},
		Realms:     []Realm{},
		Friends:    []Friend{},
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
			result.Featured = append(result.Featured, Server{
				Name: server.Name, Address: server.Address, Caption: server.Caption,
				imageURL: server.thumbnailURL,
			})
		}
	}
	if values, err := Gatherings(ctx, account); err != nil {
		result.Errors = append(result.Errors, "Gatherings: "+err.Error())
	} else {
		for _, experience := range values {
			result.Gatherings = append(result.Gatherings, Server{
				Name: experience.Name, Address: GatheringTargetPrefix + experience.ID,
				Caption: experience.Caption, imageURL: experience.Image.URL,
			})
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
	cacheArtwork(fetchContext, filepath.Join(filepath.Dir(absolute), "catalog-images"), &result)
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

const maxArtworkBytes = 8 * 1024 * 1024

func artworkURL(item playfabcatalog.Item, games []gatherings.AvailableGame) string {
	for _, game := range games {
		if game.ImageTag == "" {
			continue
		}
		for _, image := range item.Images {
			if image.Tag == game.ImageTag && validArtworkURL(image.URL) {
				return image.URL
			}
		}
	}
	for _, image := range item.Images {
		if strings.EqualFold(image.Type, playfabcatalog.ImageTypeThumbnail) && validArtworkURL(image.URL) {
			return image.URL
		}
	}
	for _, image := range item.Images {
		if validArtworkURL(image.URL) {
			return image.URL
		}
	}
	return ""
}

// validArtworkURL accepts the shared HTTPS image URL policy.
func validArtworkURL(raw string) bool { return imagecache.ValidURL(raw) }

func cacheArtwork(ctx context.Context, directory string, result *File) {
	if result == nil {
		return
	}
	cache := artworkCache(directory)
	for _, servers := range [][]Server{result.Featured, result.Gatherings} {
		for index := range servers {
			image, err := cache.Fetch(ctx, servers[index].imageURL)
			if err == nil {
				servers[index].ImagePath = image.Path
			}
		}
	}
}

// artworkCache configures the shared downloader for catalog and profile images.
func artworkCache(directory string) *imagecache.Cache {
	return imagecache.New(directory, artworkPolicy)
}

// artworkPolicy keeps catalog paths and download limits stable across both catalog entry points.
var artworkPolicy = imagecache.Config{
	MaxBytes: maxArtworkBytes, MaxFiles: maxCachedArtwork,
	Timeout: 8 * time.Second, MaxRedirects: 9,
	UserAgent: "Cinnabar/1.0", Extension: ".img",
}

// Realms lists the Realms; the account supplies the Realms XSTS token from its shared cache.
func Realms(ctx context.Context, account *authcache.Account) ([]Realm, error) {
	if account == nil {
		return nil, errNoAccount
	}
	values, err := realms.NewClient(account, nil).Realms(ctx)
	if err != nil {
		return nil, err
	}
	result := make([]Realm, 0, len(values))
	for _, realm := range values {
		entry := Realm{
			Name:       displayName(realm.Name, "", "Realm"),
			State:      realm.State,
			Target:     fmt.Sprintf("realm_id/%d", realm.ID),
			Owner:      strings.TrimSpace(realm.Owner),
			MOTD:       strings.TrimSpace(realm.MOTD),
			WorldType:  realm.WorldType,
			MaxPlayers: realm.MaxPlayers,
			DaysLeft:   realm.DaysLeft,
			Expired:    realm.Expired,
			Member:     realm.Member,
		}
		for _, player := range realm.Players {
			if player.Online {
				entry.OnlinePlayers++
			}
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
