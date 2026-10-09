package proxy

import (
	"context"
	"errors"
	"io"
	"net"
	"net/http"
	"strings"
	"testing"

	"github.com/sandertv/gophertunnel/minecraft"
)

type transportProbeResponse func(*http.Request) (*http.Response, error)

// RoundTrip supplies a probe response without contacting a server.
func (response transportProbeResponse) RoundTrip(request *http.Request) (*http.Response, error) {
	return response(request)
}

type transportContextDialer func(context.Context, string, string) (net.Conn, error)

// DialContext records the context that reaches the UDP transport.
func (dial transportContextDialer) DialContext(ctx context.Context, network, address string) (net.Conn, error) {
	return dial(ctx, network, address)
}

func TestProbedRakNetRetainsConnectBudget(t *testing.T) {
	for _, prepared := range []bool{false, true} {
		name := "signed_out"
		if prepared {
			name = "prepared"
		}
		t.Run(name, func(t *testing.T) {
			// A distinct address keeps the process-wide probe cache independent of other tests.
			listener, err := net.ListenPacket("udp", "127.0.0.1:0")
			if err != nil {
				t.Fatal(err)
			}
			defer listener.Close()
			address := listener.LocalAddr().String()
			failure := errors.New("fixture UDP dial failed")
			deadline := make(chan bool, 1)
			network := addressedServerNetwork{minecraft.AddressNetwork{
				HTTPClient: &http.Client{Transport: transportProbeResponse(func(*http.Request) (*http.Response, error) {
					return &http.Response{StatusCode: http.StatusNotFound, Body: io.NopCloser(strings.NewReader(""))}, nil
				})},
				RakNet: minecraft.RakNet{UpstreamDialer: transportContextDialer(func(ctx context.Context, _, _ string) (net.Conn, error) {
					_, bounded := ctx.Deadline()
					deadline <- bounded
					return nil, failure
				})},
			}}
			if prepared {
				transport := newPreparedTransport(context.Background(), network, address)
				defer transport.finish(false)
				_, err = transport.DialContext(t.Context(), address)
			} else {
				_, err = network.DialContext(context.Background(), address)
			}
			if !errors.Is(err, failure) {
				t.Fatalf("dial error = %v, want the UDP dial failure", err)
			}
			if !<-deadline {
				t.Fatal("probed RakNet dial lost its connection deadline")
			}
		})
	}
}
