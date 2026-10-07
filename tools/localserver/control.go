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
	lines, _ := readCommands(ctx, stdin)
	serveCommandLines(ctx, lines, cmds)
}

// readCommands reports shutdown immediately while retaining earlier commands for startup.
func readCommands(ctx context.Context, stdin io.Reader) (<-chan string, <-chan struct{}) {
	raw := make(chan string)
	lines := make(chan string)
	stopped := make(chan struct{})
	go func() {
		defer close(raw)
		scanner := bufio.NewScanner(stdin)
		for scanner.Scan() {
			line := strings.TrimSpace(scanner.Text())
			select {
			case raw <- line:
			case <-ctx.Done():
				return
			}
			if line == "stop" {
				return
			}
		}
	}()
	go func() {
		defer close(lines)
		stoppedOpen := true
		defer func() {
			if stoppedOpen {
				close(stopped)
			}
		}()
		var pending []string
		for raw != nil || len(pending) > 0 {
			var output chan string
			var next string
			if len(pending) > 0 {
				output, next = lines, pending[0]
			}
			select {
			case <-ctx.Done():
				return
			case line, ok := <-raw:
				if !ok {
					if stoppedOpen {
						close(stopped)
						stoppedOpen = false
					}
					raw = nil
				} else {
					pending = append(pending, line)
					if line == "stop" && stoppedOpen {
						close(stopped)
						stoppedOpen = false
					}
				}
			case output <- next:
				pending[0] = ""
				pending = pending[1:]
			}
		}
	}()
	return lines, stopped
}

// startupContext cancels unfinished generation when its controller requests shutdown.
func startupContext(ctx context.Context, stopped <-chan struct{}) (context.Context, context.CancelFunc) {
	ctx, cancel := context.WithCancel(ctx)
	go func() {
		select {
		case <-ctx.Done():
		case <-stopped:
			cancel()
		}
	}()
	return ctx, cancel
}

// serveCommandLines applies queued commands until shutdown or cancellation.
func serveCommandLines(ctx context.Context, lines <-chan string, cmds commands) {
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
