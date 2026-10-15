package proxy

import (
	"net"
	"slices"

	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// withPacketObserver prepends selective observation while preserving each
// observer's interests and leaving the unrestricted PacketFunc unchanged.
func withPacketObserver(dialer minecraft.Dialer, observer minecraft.PacketObserver) minecraft.Dialer {
	if observer.Func == nil || len(observer.PacketIDs) == 0 {
		return dialer
	}
	next := dialer.PacketObserver
	if next.Func == nil || len(next.PacketIDs) == 0 {
		dialer.PacketObserver = observer
		return dialer
	}
	ids, nextIDs := packetObserverIDs(observer.PacketIDs), packetObserverIDs(next.PacketIDs)
	observe, observeNext := observer.Func, next.Func
	dialer.PacketObserver = minecraft.PacketObserver{
		PacketIDs: slices.Concat(observer.PacketIDs, next.PacketIDs),
		Func: func(header packet.Header, payload []byte, source, destination net.Addr) {
			if _, ok := ids[header.PacketID]; ok {
				observe(header, payload, source, destination)
			}
			if _, ok := nextIDs[header.PacketID]; ok {
				observeNext(header, payload, source, destination)
			}
		},
	}
	return dialer
}

// packetObserverIDs snapshots the IDs used to filter one composed callback.
func packetObserverIDs(ids []uint32) map[uint32]struct{} {
	selected := make(map[uint32]struct{}, len(ids))
	for _, id := range ids {
		selected[id] = struct{}{}
	}
	return selected
}
