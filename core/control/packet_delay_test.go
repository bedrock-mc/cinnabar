package control

import (
	"errors"
	"fmt"
	"strings"
	"testing"
	"time"
)

// fakePacketDelay stands in for the relay, refusing delays above fakeMaxDelayMS.
type fakePacketDelay struct {
	delayMS  uint32
	position *Position
}

const fakeMaxDelayMS = 1000

func (d *fakePacketDelay) SetWithPosition(delayMS uint32, _ bool) error {
	if delayMS > fakeMaxDelayMS {
		return errors.New("packet delay exceeds limit")
	}
	d.delayMS = delayMS
	return nil
}

func (d *fakePacketDelay) PositionSnapshot() (uint64, *Position) { return 0, d.position }
func (*fakePacketDelay) Lease() time.Duration                    { return 3 * time.Second }

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
	delay := new(fakePacketDelay)
	server.SetPacketDelay(delay)
	for _, invalid := range []string{`{}`, `null`, `{"delay_ms":null}`, `{"delay_ms":-1}`, `{"delay_ms":1001}`, `{"delay_ms":1,"extra":true}`, `{"delay_ms":1.5}`, `{"delay_ms":1,"show_real_position":1}`} {
		assertRPCError(t, request(invalid), -32602)
	}
	for _, value := range []uint32{0, fakeMaxDelayMS} {
		payload := string(request(fmt.Sprintf(`{"delay_ms":%d}`, value)))
		if !strings.Contains(payload, fmt.Sprintf(`"delay_ms":%d`, value)) || !strings.Contains(payload, fmt.Sprintf(`"lease_ms":%d`, delay.Lease().Milliseconds())) {
			t.Fatal(payload)
		}
	}
	payload := string(request(`{"delay_ms":200,"show_real_position":true}`))
	if !strings.Contains(payload, `"session_id":0`) || strings.Contains(payload, `"position"`) {
		t.Fatalf("unavailable witness response: %s", payload)
	}
	delay.position = &Position{X: 1, Y: 2.5, Z: -3}
	payload = string(request(`{"delay_ms":200,"show_real_position":true}`))
	if !strings.Contains(payload, `"position":{"x":1,"y":2.5,"z":-3}`) {
		t.Fatalf("witness response: %s", payload)
	}
}
