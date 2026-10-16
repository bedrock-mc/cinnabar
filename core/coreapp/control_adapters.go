package coreapp

import (
	"context"
	"errors"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/catalog"
	"github.com/hashimthearab/rust-mcbe/core/control"
	"github.com/hashimthearab/rust-mcbe/core/launcher"
	"github.com/hashimthearab/rust-mcbe/core/proxy"
)

// mapSlice converts each element, keeping a nil slice nil so it still encodes as null.
func mapSlice[S, T any](in []S, convert func(S) T) []T {
	if in == nil {
		return nil
	}
	out := make([]T, len(in))
	for index, value := range in {
		out[index] = convert(value)
	}
	return out
}

// sentinelPair is a control sentinel and the source sentinel it stands for.
type sentinelPair struct{ reported, wrapped error }

// classifiedError keeps a source error's message and chain while matching the control sentinels
// its source sentinels stand for.
type classifiedError struct {
	error
	sentinels *[]sentinelPair
}

func (e classifiedError) Is(target error) bool {
	for _, pair := range *e.sentinels {
		if target == pair.reported {
			return errors.Is(e.error, pair.wrapped)
		}
	}
	return false
}

func (e classifiedError) Unwrap() error { return e.error }

func classify(err error, sentinels *[]sentinelPair) error {
	if err == nil {
		return nil
	}
	return classifiedError{error: err, sentinels: sentinels}
}

// launcherControl serves the control plane's account, catalog and screen methods.
type launcherControl struct{ *launcher.Service }

func (s launcherControl) Realms(ctx context.Context) ([]control.Realm, error) {
	realms, err := s.Service.Realms(ctx)
	return mapSlice(realms, wireRealm), err
}

func (s launcherControl) Friends(ctx context.Context) ([]control.Friend, error) {
	friends, err := s.Service.Friends(ctx)
	return mapSlice(friends, func(friend catalog.Friend) control.Friend { return control.Friend(friend) }), err
}

func (s launcherControl) People(ctx context.Context) ([]control.Person, error) {
	people, err := s.Service.People(ctx)
	return mapSlice(people[:min(len(people), catalog.MaxPeople)], wirePerson), err
}

func (launcherControl) RealmInviteCode(input string) (string, error) {
	return catalog.RealmInviteCode(input)
}

func (s launcherControl) RealmMembership(ctx context.Context, code string, accept bool) (control.Realm, error) {
	realm, err := s.Service.RealmMembership(ctx, code, accept)
	return wireRealm(realm), err
}

func (s launcherControl) FeaturedServers(ctx context.Context) ([]control.FeaturedServer, error) {
	servers, err := s.Service.FeaturedServers(ctx)
	return mapSlice(servers, wireFeaturedServer), err
}

func (s launcherControl) FeaturedServersWithCounts(ctx context.Context) ([]control.FeaturedServer, error) {
	servers, err := s.Service.FeaturedServersWithCounts(ctx)
	return mapSlice(servers, wireFeaturedServer), err
}

func (s launcherControl) Profile(ctx context.Context) (control.Profile, error) {
	profile, err := s.Service.Profile(ctx)
	return wireProfile(profile), err
}

func (s launcherControl) Ping(ctx context.Context, addresses []string) []control.PingResult {
	return mapSlice(s.Service.Ping(ctx, addresses), func(result catalog.PingResult) control.PingResult {
		return control.PingResult(result)
	})
}

func (s launcherControl) Home(ctx context.Context) (control.Home, error) {
	home, err := s.Service.Home(ctx)
	return wireHome(home), err
}

func (s launcherControl) ReportMessage(ctx context.Context, event control.MessageEvent) error {
	return s.Service.ReportMessage(ctx, catalog.MessageEvent(event))
}

func wireRealm(realm catalog.Realm) control.Realm { return control.Realm(realm) }

func wireImage(image catalog.Image) control.Image { return control.Image(image) }

func wirePerson(person catalog.Person) control.Person {
	return control.Person{XUID: person.XUID, Gamertag: person.Gamertag, Online: person.Online, Gamerpic: wireImage(person.Gamerpic)}
}

func wireFeaturedServer(server catalog.FeaturedServer) control.FeaturedServer {
	return control.FeaturedServer{
		Name: server.Name, Group: server.Group, PlayerCount: server.PlayerCount, Address: server.Address,
		Caption: server.Caption, Description: server.Description, NewsTitle: server.NewsTitle, News: server.News,
		Logo: wireImage(server.Logo), Background: wireImage(server.Background),
		Screenshots: mapSlice(server.Screenshots, wireImage),
		Games: mapSlice(server.Games, func(game catalog.Game) control.Game {
			return control.Game{Title: game.Title, Subtitle: game.Subtitle, Description: game.Description, Image: wireImage(game.Image)}
		}),
		Tags: server.Tags,
	}
}

func wireProfile(profile catalog.Profile) control.Profile {
	wire := control.Profile{
		Gamertag: profile.Gamertag, XUID: profile.XUID, Gamerpic: wireImage(profile.Gamerpic),
		Avatar: wireImage(profile.Avatar), AvatarError: profile.AvatarError,
		FeaturedScreenshot: wireImage(profile.FeaturedScreenshot), FeaturedScreenshotError: profile.FeaturedScreenshotError,
		RealName: profile.RealName, PresenceText: profile.PresenceText,
		Gamerscore: profile.Gamerscore, Friends: profile.Friends, Followers: profile.Followers,
		Statistics: (*control.ProfileStatistics)(profile.Statistics),
	}
	if achievements := profile.Achievements; achievements != nil {
		wire.Achievements = &control.ProfileAchievements{
			Unlocked: achievements.Unlocked, Total: achievements.Total,
			CurrentGamerscore: achievements.CurrentGamerscore, MaxGamerscore: achievements.MaxGamerscore,
			Entries: mapSlice(achievements.Entries, func(entry catalog.ProfileAchievement) control.ProfileAchievement {
				return control.ProfileAchievement{
					ID: entry.ID, Name: entry.Name, Description: entry.Description, Image: wireImage(entry.Image),
					Gamerscore: entry.Gamerscore, Locked: entry.Locked, DateUnlocked: entry.DateUnlocked, SuggestedOrder: entry.SuggestedOrder,
				}
			}),
		}
	}
	return wire
}

func wireHome(home catalog.Home) control.Home {
	return control.Home{
		Messages: mapSlice(home.Messages, wireMessage),
		Inbox: control.Inbox{
			Total: home.Inbox.Total, Unread: home.Inbox.Unread,
			Categories: mapSlice(home.Inbox.Categories, func(category catalog.InboxCategory) control.InboxCategory {
				return control.InboxCategory(category)
			}),
		},
		Treatments: home.Treatments, RealmInvites: home.RealmInvites, PersonaHead: wireImage(home.PersonaHead), Errors: home.Errors,
	}
}

func wireMessage(message catalog.Message) control.Message {
	return control.Message{
		Colors: message.Colors, Sender: message.Sender, ID: message.ID, InstanceID: message.InstanceID,
		ReportID: message.ReportID, Surface: message.Surface, Template: message.Template, Category: message.Category,
		Status: message.Status, Received: message.Received, Header: message.Header, Body: message.Body,
		SubTitle: message.SubTitle, Banner: message.Banner,
		Images: mapSlice(message.Images, func(image catalog.MessageImage) control.MessageImage {
			return control.MessageImage{ID: image.ID, Image: wireImage(image.Image)}
		}),
		Buttons: mapSlice(message.Buttons, func(button catalog.MessageButton) control.MessageButton {
			return control.MessageButton(button)
		}),
	}
}

// wirePackAdmission is the Status v1 view of one resource-pack admission attempt.
func wirePackAdmission(snapshot proxy.ResourcePackAdmissionSnapshot) control.PackAdmission {
	return control.PackAdmission{
		AttemptID: snapshot.AttemptID, Offer: string(snapshot.Offer), PackCount: snapshot.PackCount,
		TotalBytes: snapshot.TotalBytes, Acquisition: string(snapshot.Acquisition),
		CacheLoads: snapshot.CacheLoads, CacheHits: snapshot.CacheHits, CacheMisses: snapshot.CacheMisses,
		CacheStores: snapshot.CacheStores, CacheErrors: snapshot.CacheErrors,
		DownstreamOutcome: string(snapshot.DownstreamOutcome), Application: snapshot.Application,
	}
}

func wireConnectProgress(progress proxy.ConnectProgress) control.ConnectProgress {
	return control.ConnectProgress{
		Stage: string(progress.Stage), PacksDone: progress.PacksDone, PacksTotal: progress.PacksTotal,
		ReceivedBytes: progress.ReceivedBytes, TotalBytes: progress.TotalBytes,
	}
}

// packetDelayControl exposes the relay's packet delay to packet_delay.v1.
type packetDelayControl struct{ *proxy.PacketDelay }

func (d packetDelayControl) PositionSnapshot() (uint64, *control.Position) {
	session, position := d.PacketDelay.PositionSnapshot()
	return session, (*control.Position)(position)
}

func (packetDelayControl) Lease() time.Duration { return proxy.PacketDelayLease }
