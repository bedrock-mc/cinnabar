package main

import (
	"bufio"
	"context"
	"github.com/df-mc/dragonfly/server/world"
	"io"
	"strings"
)

// commands are the actions of the stdin protocol. reload is nil without Experiences, and its
// lines are then ignored like any unknown line.
type commands struct {
	pause  func(paused bool)
	reload func(id string)
}

// serveCommands runs the stdin protocol until "stop", EOF or ctx ends.
func serveCommands(ctx context.Context, stdin io.Reader, cmds commands) {
	lines := make(chan string)
	go func() {
		defer close(lines)
		scanner := bufio.NewScanner(stdin)
		for scanner.Scan() {
			lines <- strings.TrimSpace(scanner.Text())
		}
	}()
	for {
		select {
		case <-ctx.Done():
			return
		case line, ok := <-lines:
			if !ok || line == "stop" {
				return
			}
			switch line {
			case "pause":
				cmds.pause(true)
			case "resume":
				cmds.pause(false)
			default:
				if id, ok := reloadID(line); ok && cmds.reload != nil {
					cmds.reload(id)
				}
			}
		}
	}
}

// reloadID returns the id of an "experience reload <id>" line.
func reloadID(line string) (string, bool) {
	fields := strings.Fields(line)
	if len(fields) != 3 || fields[0] != "experience" || fields[1] != "reload" {
		return "", false
	}
	return fields[2], true
}

// setPaused suspends every dimension's simulation; connected players stay connected.
func setPaused(worlds []*world.World, paused bool) {
	for _, w := range worlds {
		w.SetPaused(paused)
	}
}
