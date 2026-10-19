package proxy

import (
	"bytes"
	"net"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

func TestVirtualContainerTranslation(t *testing.T) {
	local, peer := net.Pipe()
	session := newSessionConn(streamnet.NewFramedConn(local))
	defer session.Close()
	defer peer.Close()

	// 1. Server sends ContainerOpen with ContainerType = 255 (virtual)
	openPk := &packet.ContainerOpen{
		WindowID:                9,
		ContainerType:           255,
		ContainerPosition:       protocol.BlockPos{0, 0, 0},
		ContainerEntityUniqueID: 51003,
	}
	buf := new(bytes.Buffer)
	(&packet.Header{PacketID: openPk.ID()}).Write(buf)
	openPk.Marshal(minecraft.DefaultProtocol.NewWriter(buf, 0))

	if err := session.WritePacketRaw(buf.Bytes()); err != nil {
		t.Fatalf("WritePacketRaw failed: %v", err)
	}

	session.virtualWindowsMu.Lock()
	isVirtual := session.virtualWindows[9]
	session.virtualWindowsMu.Unlock()
	if !isVirtual {
		t.Fatalf("expected window 9 to be tracked as virtual")
	}

	// Verify that the packet written into pending has ContainerType = 0
	session.writeMu.Lock()
	pending := append([]byte(nil), session.pending...)
	session.writeMu.Unlock()

	packets, err := splitSessionBatch(pending[1:])
	if err != nil {
		t.Fatalf("splitSessionBatch failed: %v", err)
	}
	if len(packets) != 1 {
		t.Fatalf("expected 1 packet in batch, got %d", len(packets))
	}
	decodedOpen, ok := decodeSessionPacket(session.clientPool, packets[0], 0, false).(*packet.ContainerOpen)
	if !ok {
		t.Fatalf("expected ContainerOpen packet, got %T", decodedOpen)
	}
	if decodedOpen.ContainerType != 0 {
		t.Fatalf("expected ContainerType to be rewritten to 0, got %d", decodedOpen.ContainerType)
	}

	// 2. Client sends ContainerClose for window 9 with ContainerType = 0
	closePk := &packet.ContainerClose{
		WindowID:      9,
		ContainerType: 0,
		ServerSide:    false,
	}
	closeBuf := new(bytes.Buffer)
	(&packet.Header{PacketID: closePk.ID()}).Write(closeBuf)
	closePk.Marshal(minecraft.DefaultProtocol.NewWriter(closeBuf, 0))

	// Format client frame
	var frame []byte
	frame = append(frame, bridgecontract.SessionKindBatch)
	frame = appendSessionBatchPacket(frame, closeBuf.Bytes())

	peerFramed := streamnet.NewFramedConn(peer)
	go func() {
		_, _ = peerFramed.Write(frame)
	}()

	rawBatch, err := session.ReadBatchRaw(func(id uint32) bool { return id == packet.IDContainerClose })
	if err != nil {
		t.Fatalf("ReadBatchRaw failed: %v", err)
	}
	if len(rawBatch) != 1 {
		t.Fatalf("expected 1 packet from ReadBatchRaw, got %d", len(rawBatch))
	}
	decodedClose, ok := decodeSessionPacket(session.serverPool, rawBatch[0].Data, 0, false).(*packet.ContainerClose)
	if !ok {
		t.Fatalf("expected ContainerClose packet, got %T", decodedClose)
	}
	if decodedClose.ContainerType != 255 {
		t.Fatalf("expected client ContainerClose ContainerType to be rewritten to 255, got %d", decodedClose.ContainerType)
	}

	session.virtualWindowsMu.Lock()
	stillVirtual := session.virtualWindows[9]
	session.virtualWindowsMu.Unlock()
	if stillVirtual {
		t.Fatalf("expected window 9 to no longer be tracked as virtual after close")
	}

	// 3. Server sends ContainerOpen then server sends ContainerClose
	openPk2 := &packet.ContainerOpen{
		WindowID:      10,
		ContainerType: 255,
	}
	buf2 := new(bytes.Buffer)
	(&packet.Header{PacketID: openPk2.ID()}).Write(buf2)
	openPk2.Marshal(minecraft.DefaultProtocol.NewWriter(buf2, 0))
	if err := session.WritePacketRaw(buf2.Bytes()); err != nil {
		t.Fatalf("WritePacketRaw failed: %v", err)
	}

	serverClosePk := &packet.ContainerClose{
		WindowID:      10,
		ContainerType: 255,
		ServerSide:    true,
	}
	closeBuf2 := new(bytes.Buffer)
	(&packet.Header{PacketID: serverClosePk.ID()}).Write(closeBuf2)
	serverClosePk.Marshal(minecraft.DefaultProtocol.NewWriter(closeBuf2, 0))
	if err := session.WritePacketRaw(closeBuf2.Bytes()); err != nil {
		t.Fatalf("WritePacketRaw failed: %v", err)
	}

	session.virtualWindowsMu.Lock()
	stillVirtual2 := session.virtualWindows[10]
	session.virtualWindowsMu.Unlock()
	if stillVirtual2 {
		t.Fatalf("expected window 10 to no longer be tracked as virtual after server close")
	}
}
