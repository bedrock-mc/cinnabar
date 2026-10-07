package control

import (
	"fmt"
	"strings"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/proxy"
)

func TestPacketDelayControl(t *testing.T) {
	dir := t.TempDir()
	server, err := Start(dir, NewStore())
	if err != nil {
		t.Fatal(err)
	}
	defer server.Close()
	request := func(params string) []byte {
		return exchange(t, dir, []byte(fmt.Sprintf(`{"jsonrpc":"2.0","id":1,"method":"packet_delay.v1","params":%s}`, params)))
	}
	assertRPCError(t, request(`{"delay_ms":1}`), -32601)
	server.SetPacketDelay(new(proxy.PacketDelay))
	for _, invalid := range []string{`{}`, `null`, `{"delay_ms":null}`, `{"delay_ms":-1}`, `{"delay_ms":1001}`, `{"delay_ms":1,"extra":true}`, `{"delay_ms":1.5}`, `{"delay_ms":1,"show_real_position":1}`} {
		assertRPCError(t, request(invalid), -32602)
	}
	for _, value := range []uint32{0, proxy.MaxPacketDelayMS} {
		payload := string(request(fmt.Sprintf(`{"delay_ms":%d}`, value)))
		if !strings.Contains(payload, fmt.Sprintf(`"delay_ms":%d`, value)) || !strings.Contains(payload, fmt.Sprintf(`"lease_ms":%d`, proxy.PacketDelayLease.Milliseconds())) {
			t.Fatal(payload)
		}
	}
	payload := string(request(`{"delay_ms":200,"show_real_position":true}`))
	if !strings.Contains(payload, `"session_id":0`) || strings.Contains(payload, `"position"`) {
		t.Fatalf("unavailable witness response: %s", payload)
	}
}
