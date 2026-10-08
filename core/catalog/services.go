package catalog

import (
	"context"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
)

// Friends lists the friends' worlds the account can join.
func Friends(ctx context.Context, account *authcache.Account) ([]Friend, error) {
	xbl, err := XboxClient(ctx, account)
	if err != nil {
		return nil, err
	}
	defer xbl.Close()
	return fetchFriends(ctx, xbl)
}

// Gamertag returns the signed-in account's gamertag.
func Gamertag(ctx context.Context, account *authcache.Account) (string, error) {
	xbl, err := XboxClient(ctx, account)
	if err != nil {
		return "", err
	}
	defer xbl.Close()
	return xbl.UserInfo().GamerTag, nil
}
