package catalog

import (
	"context"
	"errors"
	"fmt"
	"strings"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/sandertv/gophertunnel/minecraft/realms"
)

// RealmInviteCode extracts the final link segment and admits only an opaque code, never a URL path.
func RealmInviteCode(input string) (string, error) {
	code := strings.TrimSpace(input)
	if len(code) > 255 {
		return "", errors.New("catalog: invalid Realm invitation")
	}
	if i := strings.LastIndexByte(code, '/'); i >= 0 {
		code = strings.TrimSpace(code[i+1:])
	}
	if code == "" {
		return "", errors.New("catalog: invalid Realm invitation")
	}
	for _, c := range code {
		if !(c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' || c >= '0' && c <= '9' || c == '-' || c == '_') {
			return "", errors.New("catalog: invalid Realm invitation")
		}
	}
	return code, nil
}

// RealmMembership previews an invitation, or accepts it only after the player confirms.
func RealmMembership(ctx context.Context, account *authcache.Account, code string, accept bool) (Realm, error) {
	client, err := RealmsClient(ctx, account)
	if err != nil {
		return Realm{}, err
	}
	return realmMembership(ctx, client, code, accept)
}

// membershipClient contains the two distinct service operations used by the invitation flow.
type membershipClient interface {
	Realm(context.Context, string) (realms.Realm, error)
	AcceptRealmInviteCode(context.Context, string) (realms.Realm, error)
}

// realmMembership maps a verified service result without resolving an address or starting a game connection.
func realmMembership(ctx context.Context, client membershipClient, input string, accept bool) (Realm, error) {
	code, err := RealmInviteCode(input)
	if err != nil {
		return Realm{}, err
	}
	if err := ctx.Err(); err != nil {
		return Realm{}, err
	}
	var realm realms.Realm
	if accept {
		realm, err = client.AcceptRealmInviteCode(ctx, code)
	} else {
		realm, err = client.Realm(ctx, code)
	}
	if err != nil {
		return Realm{}, err
	}
	if err := ctx.Err(); err != nil {
		return Realm{}, err
	}
	if realm.ID <= 0 {
		return Realm{}, errors.New("catalog: Realm invitation returned no Realm")
	}
	entry := realmCard(realm)
	entry.Member = true
	return entry, nil
}

// realmCard maps service fields to the launcher catalog without fetching a join address.
func realmCard(realm realms.Realm) Realm {
	entry := Realm{
		Name: displayName(realm.Name, "", "Realm"), State: realm.State,
		Target: fmt.Sprintf("realm_id/%d", realm.ID), Owner: strings.TrimSpace(realm.Owner),
		MOTD: strings.TrimSpace(realm.MOTD), WorldType: realm.WorldType,
		MaxPlayers: realm.MaxPlayers, DaysLeft: realm.DaysLeft, Expired: realm.Expired, Member: realm.Member,
	}
	for _, player := range realm.Players {
		if player.Online {
			entry.OnlinePlayers++
		}
	}
	return entry
}
