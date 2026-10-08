package authflow

import (
	"context"
	"errors"
	"fmt"
	"time"

	"github.com/sandertv/gophertunnel/minecraft/auth"
	"golang.org/x/oauth2"
)

// DeviceFlow runs Microsoft device authentication. Unset operations use the Android client.
type DeviceFlow struct {
	Authorize func(context.Context) (*oauth2.DeviceAuthResponse, error)
	Token     func(context.Context, *oauth2.DeviceAuthResponse) (*oauth2.Token, error)
}

// Request validates and publishes a device prompt before polling for its token.
// Provider error details are discarded because they may contain credentials.
func (f DeviceFlow) Request(ctx context.Context, publish func(*oauth2.DeviceAuthResponse) error) (*oauth2.Token, error) {
	if f.Authorize == nil {
		f.Authorize = auth.AndroidConfig.DeviceAuth
	}
	if f.Token == nil {
		f.Token = auth.AndroidConfig.DeviceAccessToken
	}
	response, err := f.Authorize(ctx)
	if err != nil {
		return nil, fmt.Errorf("%w: start", errDeviceAuthorization)
	}
	if err := validatePrompt(response); err != nil {
		return nil, fmt.Errorf("%w: invalid prompt", errDeviceAuthorization)
	}
	if err := publish(response); err != nil {
		return nil, fmt.Errorf("%w: publish prompt", errDeviceAuthorization)
	}
	token, err := f.Token(ctx, response)
	if err != nil {
		if deviceCodeExpired(ctx, response, err) {
			return nil, fmt.Errorf("%w: %w", errDeviceAuthorization, errDeviceCodeExpired)
		}
		return nil, fmt.Errorf("%w: complete", errDeviceAuthorization)
	}
	return token, nil
}

// deviceCodeExpired distinguishes device expiry from unrelated caller timeouts without retaining provider details.
func deviceCodeExpired(ctx context.Context, response *oauth2.DeviceAuthResponse, err error) bool {
	var providerError *oauth2.RetrieveError
	if errors.As(err, &providerError) && providerError.ErrorCode == "expired_token" {
		return true
	}
	if !errors.Is(err, context.DeadlineExceeded) || response.Expiry.IsZero() || time.Now().Before(response.Expiry) {
		return false
	}
	deadline, hasDeadline := ctx.Deadline()
	return !hasDeadline || !deadline.Before(response.Expiry)
}
