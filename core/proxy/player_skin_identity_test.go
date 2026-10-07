package proxy

import (
	"bytes"
	"context"
	"errors"
	"io"
	"reflect"
	"testing"

	"github.com/google/uuid"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

func TestRelayPlayerSkinUsesUpstreamIdentityAndPreservesPayload(t *testing.T) {
	canonical, local := uuid.New(), uuid.New()
	tests := []struct {
		name           string
		identity       login.IdentityData
		fromDownstream bool
		want           uuid.UUID
	}{
		{
			name:           "authenticated identity",
			identity:       login.IdentityData{Identity: canonical.String(), DisplayName: "Canonical"},
			fromDownstream: true,
			want:           canonical,
		},
		{
			name:           "identity without display name",
			identity:       login.IdentityData{Identity: canonical.String()},
			fromDownstream: true,
			want:           canonical,
		},
		{
			name:           "already canonical identity",
			identity:       login.IdentityData{Identity: local.String()},
			fromDownstream: true,
			want:           local,
		},
		{
			name:           "missing upstream identity",
			identity:       login.IdentityData{DisplayName: "Canonical"},
			fromDownstream: true,
			want:           local,
		},
		{
			name:           "invalid upstream identity",
			identity:       login.IdentityData{Identity: "invalid", DisplayName: "Canonical"},
			fromDownstream: true,
			want:           local,
		},
		{
			name:           "server skin stays unchanged",
			identity:       login.IdentityData{Identity: canonical.String(), DisplayName: "Canonical"},
			fromDownstream: false,
			want:           local,
		},
	}
	for _, test := range tests {
		for _, delayedReader := range []bool{false, true} {
			name := test.name + "/raw"
			if delayedReader {
				name = test.name + "/delay enabled"
			}
			t.Run(name, func(t *testing.T) {
				t.Parallel()
				down, up := newFakeDownstream(nil), newFakeUpstream(nil)
				t.Cleanup(func() { _ = down.Close(); _ = up.Close() })
				up.identity = test.identity
				wrapped := observeDisconnects(observeTransfers(up, new(TransferState), nil), func(DisconnectInfo) {})
				pixels := make([]byte, 64*64*4)
				copy(pixels, []byte{17, 35, 89, 255})
				original := &packet.PlayerSkin{
					UUID: local,
					Skin: protocol.Skin{
						SkinID:            "selected",
						SkinImageWidth:    64,
						SkinImageHeight:   64,
						SkinData:          pixels,
						SkinResourcePatch: []byte(`{"geometry":{"default":"geometry.humanoid.customSlim"}}`),
						ArmSize:           protocol.ArmSizeSlim,
						Trusted:           true,
					},
					NewSkinName: "new",
					OldSkinName: "old",
				}
				originalBytes := encodeTestPacket(original)
				want := *original
				want.UUID = test.want
				wantBytes := encodeTestPacket(&want)
				before, after := &packet.SetTime{Time: 3}, &packet.NetworkStackLatency{Timestamp: 7}
				down.useBatchReads = true
				down.batchReads <- batchResult{packets: []packet.Packet{before, original, after}}
				down.batchReads <- batchResult{err: io.EOF}
				var delay *PacketDelay
				if delayedReader {
					delay = new(PacketDelay)
				}
				if err := pumpPacketsWithDelay(context.Background(), delay, down, wrapped, test.fromDownstream); !errors.Is(err, io.EOF) {
					t.Fatalf("relay error = %v, want EOF", err)
				}
				batches := up.flushedBatches()
				if len(batches) != 1 || len(batches[0]) != 3 || batches[0][0] != before || batches[0][2] != after {
					t.Fatalf("relay changed skin batch order or boundaries: %#v", batches)
				}
				got, ok := batches[0][1].(*packet.PlayerSkin)
				if !ok {
					t.Fatalf("forwarded skin packet = %T", batches[0][1])
				}
				if !bytes.Equal(encodeTestPacket(got), wantBytes) {
					t.Fatalf("skin UUID = %s, want %s; payload preserved=%t", got.UUID, test.want, reflect.DeepEqual(got.Skin, original.Skin))
				}
				if !bytes.Equal(encodeTestPacket(original), originalBytes) {
					t.Fatal("relay mutated the received skin packet")
				}
			})
		}
	}
}
