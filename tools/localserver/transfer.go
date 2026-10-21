package localserver

import (
	"net"
	"strconv"

	"github.com/df-mc/dragonfly/server/cmd"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
)

// init registers a loopback-only transfer fixture for session replacement tests.
func init() {
	cmd.Register(cmd.New("localtransfer", "Transfers to another local test server", nil, localTransfer{}))
}

type localTransfer struct {
	Port uint16 `cmd:"port"`
}

// Run emits a real server Transfer without allowing the fixture to join a remote target.
func (t localTransfer) Run(src cmd.Source, o *cmd.Output, _ *world.Tx) {
	p, ok := src.(*player.Player)
	if !ok || t.Port == 0 {
		o.Error("A player and a nonzero port are required")
		return
	}
	if err := p.Transfer(net.JoinHostPort("127.0.0.1", strconv.Itoa(int(t.Port)))); err != nil {
		o.Error(err)
	}
}
