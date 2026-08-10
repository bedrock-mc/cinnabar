package authflow

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"golang.org/x/oauth2"
)

func TestFirstTimeSequencePersistsWithoutLeakingTokens(t *testing.T) {
	const access = "access-token-sentinel"
	const refresh = "refresh-token-sentinel"
	path := filepath.Join(t.TempDir(), "auth", "token.json")
	var output bytes.Buffer
	err := Run(context.Background(), Config{
		Path: path, Writer: &output,
		DeviceAuth: func(context.Context) (*oauth2.DeviceAuthResponse, error) {
			return &oauth2.DeviceAuthResponse{VerificationURI: "https://login.example.test/device", UserCode: "ABCD-1234"}, nil
		},
		DeviceToken: func(context.Context, *oauth2.DeviceAuthResponse) (*oauth2.Token, error) {
			return validToken(access, refresh), nil
		},
		Refresh: staticRefresh,
	})
	if err != nil {
		t.Fatalf("Run() error = %v", err)
	}
	wantKinds := []string{"checking_cache", "device_code", "authenticated"}
	events := decodeEvents(t, output.Bytes())
	for index, want := range wantKinds {
		if events[index].Kind != want {
			t.Fatalf("event %d = %q, want %q", index, events[index].Kind, want)
		}
	}
	if events[2].Method != "device_code" {
		t.Fatalf("authenticated method = %q", events[2].Method)
	}
	if strings.Contains(output.String(), access) || strings.Contains(output.String(), refresh) || strings.Contains(errString(err), access) {
		t.Fatal("authentication event stream leaked token material")
	}
	contents, readErr := os.ReadFile(path)
	if readErr != nil || !bytes.Contains(contents, []byte(refresh)) {
		t.Fatalf("persisted cache missing expected injected token: err=%v", readErr)
	}
}

func TestCachedSequenceDoesNotRequestDeviceCode(t *testing.T) {
	path := filepath.Join(t.TempDir(), "token.json")
	contents, _ := json.Marshal(validToken("cached-access", "cached-refresh"))
	if err := os.WriteFile(path, append(contents, '\n'), 0o600); err != nil {
		t.Fatal(err)
	}
	var output bytes.Buffer
	err := Run(context.Background(), Config{
		Path: path, Writer: &output,
		DeviceAuth: func(context.Context) (*oauth2.DeviceAuthResponse, error) {
			t.Fatal("cached flow requested a device code")
			return nil, nil
		},
		Refresh: staticRefresh,
	})
	if err != nil {
		t.Fatal(err)
	}
	events := decodeEvents(t, output.Bytes())
	if len(events) != 2 || events[1].Kind != "authenticated" || events[1].Method != "cached" {
		t.Fatalf("events = %#v", events)
	}
}

func TestFailureIsStagedAndUnderlyingSecretIsNotPublished(t *testing.T) {
	const sentinel = "provider-secret-sentinel"
	var output bytes.Buffer
	err := Run(context.Background(), Config{
		Path: filepath.Join(t.TempDir(), "token.json"), Writer: &output,
		DeviceAuth: func(context.Context) (*oauth2.DeviceAuthResponse, error) {
			return nil, errors.New(sentinel)
		},
	})
	if err == nil {
		t.Fatal("Run() succeeded")
	}
	if strings.Contains(output.String(), sentinel) || strings.Contains(err.Error(), sentinel) {
		t.Fatal("provider error detail leaked")
	}
	events := decodeEvents(t, output.Bytes())
	if got := events[len(events)-1]; got.Kind != "error" || got.Stage != "device_code" {
		t.Fatalf("terminal event = %#v", got)
	}
}

func TestPromptValidationRejectsUnsafeValues(t *testing.T) {
	cases := []oauth2.DeviceAuthResponse{
		{VerificationURI: "http://example.test", UserCode: "GOOD"},
		{VerificationURI: "https://user@example.test", UserCode: "GOOD"},
		{VerificationURI: "https://example.test", UserCode: "bad\ncode"},
		{VerificationURI: "https://example.test", UserCode: strings.Repeat("A", 65)},
	}
	for _, response := range cases {
		if err := validatePrompt(&response); err == nil {
			t.Fatalf("validatePrompt(%#v) succeeded", response)
		}
	}
}

func TestCancellationEmitsSafeTerminalEvent(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	var output bytes.Buffer
	err := Run(ctx, Config{
		Path: filepath.Join(t.TempDir(), "token.json"), Writer: &output,
		DeviceAuth: func(context.Context) (*oauth2.DeviceAuthResponse, error) {
			return nil, context.Canceled
		},
	})
	if err == nil {
		t.Fatal("Run() succeeded")
	}
	events := decodeEvents(t, output.Bytes())
	if got := events[len(events)-1]; got.Stage != "cancelled" {
		t.Fatalf("terminal stage = %q", got.Stage)
	}
}

func TestConfiguredSourceCannotSmuggleTokenIntoEvents(t *testing.T) {
	const sentinel = "token-sentinel Microsoft token device authorization"
	var output bytes.Buffer
	err := Run(context.Background(), Config{
		Path: "ignored", Writer: &output,
		CachedSource: func(context.Context, authcache.Config) (oauth2.TokenSource, error) {
			return nil, errors.New(sentinel)
		},
	})
	if err == nil || strings.Contains(output.String(), sentinel) || strings.Contains(err.Error(), sentinel) {
		t.Fatalf("unsafe result: output=%q err=%v", output.String(), err)
	}
	events := decodeEvents(t, output.Bytes())
	if got := events[len(events)-1]; got.Stage != "cache" {
		t.Fatalf("terminal stage = %q, want cache", got.Stage)
	}
}

func validToken(access, refresh string) *oauth2.Token {
	return &oauth2.Token{AccessToken: access, RefreshToken: refresh, Expiry: time.Now().Add(time.Hour)}
}

func staticRefresh(token *oauth2.Token, _ io.Writer) oauth2.TokenSource {
	return oauth2.StaticTokenSource(token)
}

func decodeEvents(t *testing.T, data []byte) []event {
	t.Helper()
	var events []event
	decoder := json.NewDecoder(bytes.NewReader(data))
	for decoder.More() {
		var value event
		if err := decoder.Decode(&value); err != nil {
			t.Fatal(err)
		}
		events = append(events, value)
	}
	return events
}

func errString(err error) string {
	if err == nil {
		return ""
	}
	return err.Error()
}
