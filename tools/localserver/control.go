package main

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/df-mc/dragonfly/server/world"
	"io"
	"strings"
	"sync"
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

// pauseState pauses the worlds while the launcher asks or a hold such as the spawn warm-up keeps them still.
type pauseState struct {
	mu              sync.Mutex
	worlds          []*world.World
	requested, held bool
}

// request records the launcher's pause or resume.
func (p *pauseState) request(paused bool) {
	p.mu.Lock()
	defer p.mu.Unlock()
	p.requested = paused
	setPaused(p.worlds, p.requested || p.held)
}

// hold keeps the worlds paused whatever the launcher asks until released.
func (p *pauseState) hold(held bool) {
	p.mu.Lock()
	defer p.mu.Unlock()
	p.held = held
	setPaused(p.worlds, p.requested || p.held)
}

// standbyFlag starts a server with no world that waits for a standbyStart line.
const standbyFlag = "-standby"

// standbyStart prefixes the JSON array of world arguments that ends a standby wait.
const standbyStart = "start "

// awaitStandbyStart reads the line giving a standby server its world arguments, returning them and
// the rest of stdin; ok is false when stdin closes first.
func awaitStandbyStart(stdin io.Reader) (args []string, rest io.Reader, ok bool, err error) {
	reader := bufio.NewReader(stdin)
	line, err := reader.ReadString('\n')
	if errors.Is(err, io.EOF) && strings.TrimSpace(line) == "" {
		return nil, nil, false, nil
	}
	if err != nil && !errors.Is(err, io.EOF) {
		return nil, nil, false, fmt.Errorf("read standby start: %w", err)
	}
	encoded, found := strings.CutPrefix(strings.TrimSpace(line), standbyStart)
	if !found {
		return nil, nil, false, fmt.Errorf("standby expected %q, got %q", standbyStart, line)
	}
	if err := json.Unmarshal([]byte(encoded), &args); err != nil {
		return nil, nil, false, fmt.Errorf("decode standby start: %w", err)
	}
	return args, reader, true, nil
}
