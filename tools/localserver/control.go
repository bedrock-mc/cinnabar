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
	lines := readCommands(ctx, stdin)
	serveCommandLines(ctx, lines, cmds)
}

// readCommands reads controller lines until shutdown or cancellation.
func readCommands(ctx context.Context, stdin io.Reader) <-chan string {
	raw := make(chan string)
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
	return raw
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

// pauseState serialises pause and resume requests for all dimensions.
type pauseState struct {
	mu     sync.Mutex
	worlds []*world.World
}

// request applies the launcher's pause or resume to every dimension.
func (p *pauseState) request(paused bool) {
	p.mu.Lock()
	defer p.mu.Unlock()
	setPaused(p.worlds, paused)
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
