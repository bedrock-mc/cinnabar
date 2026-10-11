package proxy

import (
	"bytes"
	"errors"
	"io"
	"net"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// TestSessionTerminalTCPDrain keeps a terminal frame readable while startup writes are in flight.
func TestSessionTerminalTCPDrain(t *testing.T) {
	for _, terminal := range []packet.Packet{
		&packet.Transfer{Address: "next.example.test", Port: 19133},
		&packet.Disconnect{Message: "fixture kicked during spawn"},
	} {
		t.Run(map[uint32]string{packet.IDTransfer: "transfer", packet.IDDisconnect: "disconnect"}[terminal.ID()], func(t *testing.T) {
			listener, err := net.ListenTCP("tcp", &net.TCPAddr{IP: net.IPv4(127, 0, 0, 1)})
			if err != nil {
				t.Fatal(err)
			}
			defer listener.Close()
			peer, err := net.DialTCP("tcp", nil, listener.Addr().(*net.TCPAddr))
			if err != nil {
				t.Fatal(err)
			}
			defer peer.Close()
			if err := peer.SetDeadline(time.Now().Add(5 * time.Second)); err != nil {
				t.Fatal(err)
			}
			if err := peer.SetWriteBuffer(1024); err != nil {
				t.Fatal(err)
			}
			local, err := listener.AcceptTCP()
			if err != nil {
				t.Fatal(err)
			}
			defer local.Close()
			session := newSessionConn(streamnet.NewFramedConn(local))
			closed := make(chan error, 1)
			go func() {
				err := session.WritePacketImmediate(terminal)
				if errors.Is(err, errSessionEnded) {
					err = nil
				}
				closed <- errors.Join(err, session.Close())
			}()

			client := streamnet.NewFramedConn(peer)
			frame, err := client.ReadPacket()
			if err != nil {
				t.Fatal(err)
			}
			var want []byte
			if transfer, ok := terminal.(*packet.Transfer); ok {
				want, err = encodeSessionJSON(bridgecontract.SessionKindTransfer, sessionTransfer{Address: transfer.Address, Port: transfer.Port})
			} else {
				want, err = encodeSessionJSON(bridgecontract.SessionKindDisconnect, sessionDisconnect{Message: terminal.(*packet.Disconnect).Message})
			}
			if err != nil || !bytes.Equal(frame, want) {
				t.Fatalf("terminal frame = %q, want %q: %v", frame, want, err)
			}
			if _, err := client.ReadPacket(); !errors.Is(err, io.EOF) {
				t.Fatalf("terminal stream end = %v, want EOF", err)
			}
			// More than the send buffer can hold: success requires the core to keep reading.
			if _, err := client.Write(bytes.Repeat([]byte{bridgecontract.SessionKindBatch}, 64*1024)); err != nil {
				t.Fatalf("in-flight startup write after terminal delivery: %v", err)
			}
			select {
			case err := <-closed:
				t.Fatalf("core closed before the peer finished sending: %v", err)
			default:
			}
			if err := peer.CloseWrite(); err != nil {
				t.Fatal(err)
			}
			if err := <-closed; err != nil {
				t.Fatal(err)
			}
		})
	}
}
