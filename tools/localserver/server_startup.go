package main

import (
	"errors"
	"slices"

	"github.com/df-mc/dragonfly/server"
	"github.com/df-mc/dragonfly/server/world"
)

// newLocalServer returns the server and a cleanup path for failure before Listen.
func newLocalServer(conf server.Config) (*server.Server, func() error) {
	var listeners []server.Listener
	conf.Listeners = slices.Clone(conf.Listeners)
	for i, listen := range conf.Listeners {
		conf.Listeners[i] = func(c server.Config) (server.Listener, error) {
			l, err := listen(c)
			if err == nil {
				listeners = append(listeners, l)
			}
			return l, err
		}
	}
	srv := conf.New()
	return srv, func() error {
		var errs []error
		for _, l := range listeners {
			errs = append(errs, l.Close())
		}
		for _, w := range []*world.World{srv.End(), srv.Nether(), srv.World()} {
			errs = append(errs, w.Close())
		}
		if conf.PlayerProvider != nil {
			errs = append(errs, conf.PlayerProvider.Close())
		}
		return errors.Join(errs...)
	}
}
