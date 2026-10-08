package proxy

import (
	"context"
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"fmt"
	"net"
	"net/http"
	"strconv"
	"time"

	"github.com/df-mc/go-nethernet"
	"github.com/df-mc/go-nethernet/endpoint"
	"github.com/go-jose/go-jose/v4"
	"github.com/go-jose/go-jose/v4/jwt"
	"github.com/hashimthearab/rust-mcbe/core/localworld"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
)

// LocalTargetFunc supplies the local server's address and transport together.
type LocalTargetFunc func(context.Context) (localworld.ConnectionTarget, bool, error)

// withLocalTarget never routes a selected local world through online discovery or authentication.
func withLocalTarget(local LocalTargetFunc, online func(context.Context) (*resolvedUpstreamTarget, error)) func(context.Context) (*resolvedUpstreamTarget, error) {
	if local == nil {
		return online
	}
	return func(ctx context.Context) (*resolvedUpstreamTarget, error) {
		target, ok, err := local(ctx)
		if err != nil {
			return nil, err
		}
		if !ok {
			return online(ctx)
		}
		switch target.Transport {
		case localworld.TransportRakNet:
			return &resolvedUpstreamTarget{address: target.Address, network: minecraft.RakNet{}}, nil
		case localworld.TransportNetherNetLAN:
			return resolveLocalLANTarget(ctx, target)
		case localworld.TransportNetherNetHTTP:
			host, port, err := net.SplitHostPort(target.Address)
			ip := net.ParseIP(host)
			portNumber, portErr := strconv.ParseUint(port, 10, 16)
			isLoopback := ip != nil && ip.IsLoopback()
			if err != nil || !isLoopback {
				return nil, fmt.Errorf("local NetherNet target is not a loopback address: %q", target.Address)
			}
			if portErr != nil || portNumber == 0 {
				return nil, fmt.Errorf("local NetherNet target has invalid port: %q", target.Address)
			}
			client := endpoint.ClientConfig{HTTPClient: &http.Client{
				Timeout: 30 * time.Second,
				// Local BDS never redirects: do not let a local target forward
				// status requests or SDP offers to another origin.
				CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse },
			}}.New()
			return &resolvedUpstreamTarget{
				address: "http://" + target.Address,
				network: localNetherNetNetwork{NetherNet: minecraft.NetherNet{Signaling: client}, status: client},
			}, nil
		default:
			return nil, fmt.Errorf("unsupported local transport %q", target.Transport)
		}
	}
}

// Local BDS uses Mojang's GET status and full-ICE HTTP SDP exchange, without Xbox signaling.
type localNetherNetNetwork struct {
	minecraft.NetherNet
	status *endpoint.Client
}

func (n localNetherNetNetwork) PingContext(ctx context.Context, address string) ([]byte, error) {
	return n.status.PingContext(ctx, address)
}

// DialContext is the signed-out dial. BDS refuses HTTP offers without an identity even with
// online-mode off, but admits a self-signed one; signed-in dials present the account's instead.
func (n localNetherNetNetwork) DialContext(ctx context.Context, address string) (net.Conn, error) {
	identity, err := selfSignedIdentity(time.Now())
	if err != nil {
		return nil, err
	}
	n.Dialer.Identity = identity
	return n.NetherNet.DialContext(ctx, address)
}

// selfSignedLifetime outlives a join left waiting on the server trust question, whose redial
// presents the same identity again.
const selfSignedLifetime = time.Hour

// selfSignedIdentity carries cpk as base64 DER: BDS rejects the JWK form.
func selfSignedIdentity(now time.Time) (*nethernet.Identity, error) {
	key, err := ecdsa.GenerateKey(elliptic.P384(), rand.Reader)
	if err != nil {
		return nil, fmt.Errorf("local NetherNet identity key: %w", err)
	}
	publicKey := login.MarshalPublicKey(&key.PublicKey)
	signer, err := jose.NewSigner(jose.SigningKey{Algorithm: jose.ES384, Key: key}, (&jose.SignerOptions{}).WithHeader("x5u", publicKey))
	if err != nil {
		return nil, fmt.Errorf("local NetherNet identity signer: %w", err)
	}
	token, err := jwt.Signed(signer).Claims(struct {
		jwt.Claims
		PublicKey string `json:"cpk"`
	}{
		Claims:    jwt.Claims{IssuedAt: jwt.NewNumericDate(now), Expiry: jwt.NewNumericDate(now.Add(selfSignedLifetime))},
		PublicKey: publicKey,
	}).Serialize()
	if err != nil {
		return nil, fmt.Errorf("local NetherNet identity token: %w", err)
	}
	return &nethernet.Identity{PrivateKey: key, Token: token, Domain: "self"}, nil
}
