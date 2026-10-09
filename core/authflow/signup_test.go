package authflow

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"net/url"
	"path/filepath"
	"strings"
	"testing"

	"github.com/df-mc/go-xsapi/v2/xal/sisu"
	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"golang.org/x/oauth2"
)

// signupError supplies a signed fixture URL without contacting Microsoft.
func signupError() error {
	return fmt.Errorf("exchange: %w", &sisu.AccountCreationRequiredError{SignupURL: &url.URL{
		Scheme: "https", Host: "sisu.xboxlive.com", Path: "/signup", RawQuery: "signature=private-fixture",
	}})
}

// signupConfig supplies a cached Microsoft login and captures only auth events.
func signupConfig(t *testing.T, output *bytes.Buffer) Config {
	t.Helper()
	return Config{
		Path: filepath.Join(t.TempDir(), "token.json"), Writer: output,
		CachedSource: func(context.Context, authcache.Config) (oauth2.TokenSource, error) {
			return oauth2.StaticTokenSource(validToken("access", "refresh")), nil
		},
	}
}

func TestXboxSignupResumesWithSameMicrosoftLogin(t *testing.T) {
	var output bytes.Buffer
	config := signupConfig(t, &output)
	attempts := 0
	var original oauth2.TokenSource
	config.CompleteSignIn = func(_ context.Context, _ string, source oauth2.TokenSource) error {
		attempts++
		if attempts == 1 {
			original = source
			return signupError()
		}
		if source != original || strings.Contains(output.String(), `"authenticated"`) {
			t.Fatal("signup restarted Microsoft login or reported success too early")
		}
		return nil
	}
	if err := Run(context.Background(), config); err != nil {
		t.Fatal(err)
	}
	events := decodeEvents(t, output.Bytes())
	if attempts != 2 || len(events) != 3 || events[1].Kind != "xbox_signup" || events[1].SignupURL == "" || events[2].Kind != "authenticated" {
		t.Fatalf("attempts=%d events=%+v", attempts, events)
	}
}

func TestXboxSignupFailureAndCancellationNeverAuthenticate(t *testing.T) {
	for _, cancelled := range []bool{false, true} {
		t.Run(fmt.Sprint(cancelled), func(t *testing.T) {
			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			var output bytes.Buffer
			config := signupConfig(t, &output)
			attempts := 0
			config.CompleteSignIn = func(context.Context, string, oauth2.TokenSource) error {
				attempts++
				if attempts == 1 {
					return signupError()
				}
				if cancelled {
					cancel()
				}
				return errors.New("provider-secret-sentinel")
			}
			err := Run(ctx, config)
			events := decodeEvents(t, output.Bytes())
			last := events[len(events)-1]
			if err == nil || last.Kind != "error" || strings.Contains(output.String(), `"authenticated"`) || strings.Contains(output.String(), "provider-secret-sentinel") || strings.Contains(errString(err), "private-fixture") {
				t.Fatalf("err=%v last=%+v", err, last)
			}
			if cancelled && last.Stage != "cancelled" {
				t.Fatalf("cancellation stage=%s", last.Stage)
			}
		})
	}
}

func TestInvalidXboxSignupURLFailsWithoutPublishingIt(t *testing.T) {
	var output bytes.Buffer
	config := signupConfig(t, &output)
	config.CompleteSignIn = func(context.Context, string, oauth2.TokenSource) error {
		return &sisu.AccountCreationRequiredError{SignupURL: &url.URL{Scheme: "file", Path: "/private-fixture"}}
	}
	if err := Run(context.Background(), config); err == nil || strings.Contains(output.String(), "private-fixture") || strings.Contains(output.String(), `"authenticated"`) {
		t.Fatalf("err=%v events=%s", err, output.String())
	}
}
