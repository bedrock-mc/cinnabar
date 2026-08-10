// Package authflow exposes Microsoft device authentication as a bounded JSONL
// event stream. OAuth tokens remain owned by the Go core and are never encoded
// into an event or error returned to the launcher.
package authflow

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/url"
	"strings"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/sandertv/gophertunnel/minecraft/auth"
	"golang.org/x/oauth2"
)

const maxEventBytes = 4096

// Config supplies the cache and injectable authentication operations.
type Config struct {
	Path         string
	Writer       io.Writer
	DeviceAuth   func(context.Context) (*oauth2.DeviceAuthResponse, error)
	DeviceToken  func(context.Context, *oauth2.DeviceAuthResponse) (*oauth2.Token, error)
	Refresh      func(*oauth2.Token, io.Writer) oauth2.TokenSource
	CachedSource func(context.Context, authcache.Config) (oauth2.TokenSource, error)
}

type event struct {
	Version         int    `json:"v"`
	Kind            string `json:"event"`
	VerificationURI string `json:"verification_uri,omitempty"`
	UserCode        string `json:"user_code,omitempty"`
	Method          string `json:"method,omitempty"`
	Stage           string `json:"stage,omitempty"`
	Message         string `json:"message,omitempty"`
}

// Run validates or acquires a token, persists it through authcache, emits a
// terminal authenticated event, and then discards the token source.
func Run(ctx context.Context, config Config) error {
	writer := config.Writer
	if writer == nil {
		writer = io.Discard
	}
	if config.Path == "" {
		return fail(writer, "configuration", "Authentication cache is not configured.")
	}
	if err := emit(writer, event{Version: 1, Kind: "checking_cache"}); err != nil {
		return err
	}
	deviceAuth := config.DeviceAuth
	if deviceAuth == nil {
		deviceAuth = auth.AndroidConfig.DeviceAuth
	}
	deviceToken := config.DeviceToken
	if deviceToken == nil {
		deviceToken = auth.AndroidConfig.DeviceAccessToken
	}
	refresh := config.Refresh
	if refresh == nil {
		refresh = auth.AndroidConfig.RefreshTokenSourceWriter
	}
	acquired := false
	request := func(ctx context.Context, _ io.Writer) (*oauth2.Token, error) {
		response, err := deviceAuth(ctx)
		if err != nil {
			return nil, errors.New("start device authorization")
		}
		if err := validatePrompt(response); err != nil {
			return nil, err
		}
		if err := emit(writer, event{
			Version: 1, Kind: "device_code", VerificationURI: response.VerificationURI,
			UserCode: response.UserCode,
		}); err != nil {
			return nil, errors.New("publish device authorization")
		}
		token, err := deviceToken(ctx, response)
		if err != nil {
			return nil, errors.New("complete device authorization")
		}
		acquired = true
		return token, nil
	}
	source := config.CachedSource
	if source == nil {
		source = authcache.Source
	}
	_, err := source(ctx, authcache.Config{
		Path: config.Path, Writer: io.Discard, Request: request, Refresh: refresh,
	})
	if err != nil {
		stage := "cache"
		message := "Could not validate the saved account."
		if strings.Contains(err.Error(), "device authorization") || strings.Contains(err.Error(), "Microsoft token") {
			stage = "device_code"
			message = "Microsoft sign-in did not complete. Try again."
		}
		if errors.Is(ctx.Err(), context.Canceled) {
			stage, message = "cancelled", "Sign-in was cancelled."
		}
		return fail(writer, stage, message)
	}
	method := "cached"
	if acquired {
		method = "device_code"
	}
	return emit(writer, event{Version: 1, Kind: "authenticated", Method: method})
}

func validatePrompt(response *oauth2.DeviceAuthResponse) error {
	if response == nil || len(response.UserCode) == 0 || len(response.UserCode) > 64 {
		return errors.New("device authorization returned an invalid code")
	}
	for _, character := range response.UserCode {
		if character < 0x21 || character > 0x7e {
			return errors.New("device authorization returned an invalid code")
		}
	}
	parsed, err := url.Parse(response.VerificationURI)
	if err != nil || parsed.Scheme != "https" || parsed.Host == "" || parsed.User != nil {
		return errors.New("device authorization returned an invalid URL")
	}
	return nil
}

func fail(writer io.Writer, stage, message string) error {
	if err := emit(writer, event{Version: 1, Kind: "error", Stage: stage, Message: message}); err != nil {
		return err
	}
	return fmt.Errorf("authentication %s: %s", stage, message)
}

func emit(writer io.Writer, value event) error {
	line, err := json.Marshal(value)
	if err != nil {
		return errors.New("encode authentication event")
	}
	if len(line)+1 > maxEventBytes {
		return errors.New("authentication event exceeds size limit")
	}
	line = append(line, '\n')
	for len(line) != 0 {
		written, err := writer.Write(line)
		if err != nil || written <= 0 || written > len(line) {
			return errors.New("write authentication event")
		}
		line = line[written:]
	}
	return nil
}
