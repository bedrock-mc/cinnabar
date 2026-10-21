package proxy

import (
	"context"
	"encoding/json"
	"fmt"
	"testing"
	"time"

	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/device"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// TestSessionLoginRetainsEmptyProfileHash checks PocketMine's required classic-skin claim upstream.
func TestSessionLoginRetainsEmptyProfileHash(t *testing.T) {
	request := testSessionConnect(t)
	var claims map[string]json.RawMessage
	if err := json.Unmarshal(request.ClientData, &claims); err != nil {
		t.Fatal(err)
	}
	claims["ProfileHash"] = json.RawMessage(`""`)
	var err error
	request.ClientData, err = json.Marshal(claims)
	if err != nil {
		t.Fatal(err)
	}
	profile := device.New(protocol.DeviceAndroid)
	downstream, err := newSessionDownstream(request, &profile)
	if err != nil {
		t.Fatal(err)
	}
	listener, network := newAdmissionTestListener(t, nil)
	done := make(chan error, 1)
	go func() {
		accepted, err := listener.Accept()
		if err == nil {
			conn := accepted.(*minecraft.Conn)
			defer conn.Close()
			encoded, marshalErr := json.Marshal(conn.ClientData())
			err = marshalErr
			var received map[string]json.RawMessage
			if err == nil {
				err = json.Unmarshal(encoded, &received)
			}
			if err == nil && string(received["ProfileHash"]) != `""` {
				err = fmt.Errorf("upstream ProfileHash = %s, want an explicit empty string", received["ProfileHash"])
			}
			if err == nil {
				err = conn.WritePacketImmediate(&packet.StartGame{EntityRuntimeID: 9})
				if err == nil {
					_, _ = conn.ReadPacket()
				}
			}
		}
		done <- err
	}()
	ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
	defer cancel()
	dialer := newUpstreamDialerForAdmission(downstream, nil, nil, nil, false)
	conn, err := dialer.DialContextNetwork(ctx, network, "")
	if err != nil {
		_ = listener.Close()
		t.Fatalf("dial: %v; server: %v", err, <-done)
	}
	_ = conn.Close()
	if err := <-done; err != nil {
		t.Fatal(err)
	}
}
