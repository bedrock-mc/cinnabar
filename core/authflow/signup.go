package authflow

import (
	"context"
	"errors"
	"io"
	"time"

	"github.com/df-mc/go-xsapi/v2/xal/sisu"
	"golang.org/x/oauth2"
)

const (
	xboxSignupTimeout = 20 * time.Minute
	xboxSignupRetry   = 3 * time.Second
)

// completeXboxSignIn offers Microsoft's hosted Xbox signup when the saved Microsoft account
// has no profile, then retries with the same credentials until signup succeeds or is cancelled.
func completeXboxSignIn(ctx context.Context, config Config, source oauth2.TokenSource, writer io.Writer) error {
	exchange := func(attemptCtx context.Context) error {
		attempt, cancel := context.WithTimeout(attemptCtx, completeSignInTimeout)
		defer cancel()
		return config.CompleteSignIn(attempt, config.Path, source)
	}
	err := exchange(ctx)
	if ctx.Err() != nil {
		return fail(writer, "cancelled", "Sign-in was cancelled.")
	}
	var signup *sisu.AccountCreationRequiredError
	if !errors.As(err, &signup) {
		return nil // Ordinary warmup failures are retried by the first join.
	}
	if signup == nil || signup.SignupURL == nil || signup.SignupURL.Scheme != "https" || signup.SignupURL.Host == "" || signup.SignupURL.User != nil {
		return fail(writer, "xbox_signup", "Could not open Xbox profile setup. Try signing in again.")
	}
	// The signed URL goes only to the sign-in UI and browser, never to diagnostics or errors.
	if err := emit(writer, event{Version: 1, Kind: "xbox_signup", SignupURL: signup.SignupURL.String()}); err != nil {
		return err
	}
	wait, cancel := context.WithTimeout(ctx, xboxSignupTimeout)
	defer cancel()
	ticker := time.NewTicker(xboxSignupRetry)
	defer ticker.Stop()
	for {
		select {
		case <-wait.Done():
			if errors.Is(wait.Err(), context.Canceled) {
				return fail(writer, "cancelled", "Sign-in was cancelled.")
			}
			return fail(writer, "xbox_signup", "Xbox profile setup timed out. Try signing in again.")
		case <-ticker.C:
		}
		err = exchange(wait)
		if wait.Err() != nil {
			continue
		}
		if err == nil {
			return nil
		}
		if !errors.As(err, &signup) {
			return fail(writer, "xbox_signup", "Could not finish Xbox profile setup. Try signing in again.")
		}
	}
}
