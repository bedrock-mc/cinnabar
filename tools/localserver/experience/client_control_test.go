package experience

import (
	"crypto/ed25519"
	"encoding/json"
	"os"
	"strings"
	"sync"
	"testing"
	"testing/synctest"
	"time"

	"github.com/df-mc/dragonfly/server"
	playerpkg "github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/session"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/google/uuid"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"

	ext "github.com/cinnabar-mc/cinnabar/tools/localserver/cinnabarext"
)

// controlConn supplies only the packet and identity methods used by the extension transport.
type controlConn struct {
	session.Conn
	player uuid.UUID
	input  []packet.Packet
	mu     sync.Mutex
	output []*packet.ScriptMessage
}

// IdentityData binds the handshake to the fixture's authenticated player.
func (c *controlConn) IdentityData() login.IdentityData {
	return login.IdentityData{Identity: c.player.String()}
}

// ReadPacket delivers queued handshake packets followed by an ordinary packet.
func (c *controlConn) ReadPacket() (packet.Packet, error) {
	if len(c.input) == 0 {
		return &packet.Text{}, nil
	}
	pk := c.input[0]
	c.input = c.input[1:]
	return pk, nil
}

// WritePacket records carrier messages sent by the transport and its retry worker.
func (c *controlConn) WritePacket(pk packet.Packet) error {
	c.mu.Lock()
	defer c.mu.Unlock()
	if message, ok := pk.(*packet.ScriptMessage); ok {
		c.output = append(c.output, message)
	}
	return nil
}

// messages snapshots the carrier output without sharing the worker's slice.
func (c *controlConn) messages() []*packet.ScriptMessage {
	c.mu.Lock()
	defer c.mu.Unlock()
	return append([]*packet.ScriptMessage(nil), c.output...)
}

// controlListener supplies one connection to the real extension listener.
type controlListener struct {
	server.Listener
	conn session.Conn
}

// Accept hands the fixture connection to the extension transport.
func (l controlListener) Accept() (session.Conn, error) { return l.conn, nil }

// TestRegistryConfirmationRetriesAfterBudgetRefills exercises the committed Rust operation
// through the real retained control queue while ordinary traffic has exhausted its credit.
func TestRegistryConfirmationRetriesAfterBudgetRefills(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		raw, err := os.ReadFile("../cinnabarext/testdata/manifest_payload.json")
		if err != nil {
			t.Fatal(err)
		}
		var manifest ext.Manifest
		if err := ext.Decode(raw, &manifest); err != nil {
			t.Fatal(err)
		}
		channel := manifest.ID + ".registry"
		fingerprint := "fingerprint"
		manifest.Channels = append(manifest.Channels, ext.Channel{ID: channel, Schema: 1, Direction: ext.ToClient, Fields: []ext.Field{ext.TextField{MaxBytes: uint16(len(fingerprint))}}})
		digest := strings.Repeat("ab", 32)
		transport, err := ext.New(ext.Config{Key: ed25519.NewKeyFromSeed(make([]byte, ed25519.SeedSize)), Audience: "127.0.0.1:19132", Revision: 1, Bundles: []ext.Bundle{{Manifest: manifest, Digest: digest, Bytes: 1}}})
		if err != nil {
			t.Fatal(err)
		}
		player := uuid.New()
		inner := &controlConn{player: player}
		listener, err := transport.Listener(func(server.Config) (server.Listener, error) { return controlListener{conn: inner}, nil })(server.Config{})
		if err != nil {
			t.Fatal(err)
		}
		conn, err := listener.Accept()
		if err != nil {
			t.Fatal(err)
		}
		deliver := func(message ext.Control) {
			t.Helper()
			raw, err := ext.Encode(message)
			if err != nil {
				t.Fatal(err)
			}
			inner.input = append(inner.input, &packet.ScriptMessage{Identifier: ext.Carrier, Data: raw})
			if _, err := conn.ReadPacket(); err != nil {
				t.Fatal(err)
			}
		}
		marker := transport.Marker()
		var offer ext.Offer
		offerDigest, err := marker.Offer.Verify(marker.ServerKey, ext.OfferDomain, ext.MaxMarkerBytes/2, &offer)
		if err != nil {
			t.Fatal(err)
		}
		deliver(ext.Control{Hello: &ext.Hello{Version: ext.WireVersion, API: ext.APIVersion, Capabilities: ext.ImplementedPermissions, OfferDigest: offerDigest, ClientChallenge: strings.Repeat("c1", 32), Connection: strings.Repeat("c0", 32)}})
		var reply ext.Control
		messages := inner.messages()
		if len(messages) != 1 {
			t.Fatalf("handshake output = %d", len(messages))
		}
		if err := ext.Decode(messages[0].Data, &reply); err != nil || reply.Accept == nil {
			t.Fatalf("accept = %+v, %v", reply, err)
		}
		var accept ext.Accept
		if _, err := reply.Accept.Verify(marker.ServerKey, ext.AcceptDomain, ext.MaxPayloadBytes, &accept); err != nil {
			t.Fatal(err)
		}
		deliver(ext.Control{Ready: &ext.Ready{Session: accept.Session, Packages: []string{digest}, Generation: ext.InitialBundleGeneration, Permissions: map[string]ext.Permissions{manifest.ID: manifest.Permissions & ext.ImplementedPermissions}, WorldEpoch: 1}})
		dataChannel := manifest.Channels[0]
		number := int64(1)
		payload := []ext.Scalar{{Integer: &number}}
		sent := 0
		for transport.SendRecord(player, manifest.ID, dataChannel.ID, dataChannel.Schema, payload) {
			sent++
		}
		if sent == 0 {
			t.Fatal("ordinary traffic never acquired credit")
		}
		before := len(inner.messages())
		log, _ := testLog(t)
		h := &Host{channels: transport, log: log}
		d := &dispatcher{id: manifest.ID}
		var op SendClientOp
		raw, err = json.Marshal(map[string]any{"player": player.String(), "channel": channel, "schema": 1, "payload": []ext.Scalar{{Text: &fingerprint}}, "control": true})
		if err != nil {
			t.Fatal(err)
		}
		if err := json.Unmarshal(raw, &op); err != nil {
			t.Fatal(err)
		}
		actor := world.EntitySpawnOpts{ID: player}.New(playerpkg.Type, playerpkg.Config{Name: "Actor"})
		h.players.items = map[*world.EntityHandle]*playerState{
			actor: {connection: transport.PlayerSession(player)},
		}
		h.sendCurrent(d, event{actor: actor}, &op)
		if len(inner.messages()) != before {
			t.Fatal("confirmation bypassed the exhausted budget")
		}
		synctest.Wait()
		refilled := make(chan struct{})
		time.AfterFunc(time.Second, func() { close(refilled) })
		<-refilled
		synctest.Wait()
		messages = inner.messages()[before:]
		if len(messages) != 1 {
			t.Fatalf("delivered %d confirmations after refill, want one", len(messages))
		}
		var envelope ext.Envelope
		if err := ext.Decode(messages[0].Data, &envelope); err != nil || envelope.Channel != channel || len(envelope.Payload) != 1 || envelope.Payload[0].Text == nil || *envelope.Payload[0].Text != fingerprint {
			t.Fatalf("confirmation = %+v, %v", envelope, err)
		}
		if d.unsent.n.Load() != 0 {
			t.Fatal("retained confirmation counted as dropped")
		}
	})
}
