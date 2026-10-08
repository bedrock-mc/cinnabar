// Command fakehelper stands in for experience-runtime in the supervisor tests. It answers load,
// then each callback as FAKE_MODE scripts:
//
//   - ok: writes "callback <seq>" to stderr and commits no ops;
//   - env: writes "env" and a JSON list of its environment's names to stderr and commits no ops;
//   - hang: never reads or answers anything after load, not even shutdown;
//   - garbage: answers with a frame that is not JSON;
//   - oversized: answers with a length one byte over FAKE_MAX_FRAME;
//   - exit: exits with status 3 instead of answering;
//   - wrong_seq: commits no ops under the next seq;
//   - load_failed: answers load with load_failed and exits 1, as the runtime does.
//
// It answers load with FAKE_LOADED, the body of a loaded frame, verbatim. Shutdown and the end of
// stdin end it with status 0.
package main

import (
	"encoding/binary"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"slices"
	"strconv"
	"strings"
	"time"
)

var modes = []string{"ok", "env", "hang", "garbage", "oversized", "exit", "wrong_seq", "load_failed"}

func main() {
	mode := os.Getenv("FAKE_MODE")
	if !slices.Contains(modes, mode) {
		fail("unknown FAKE_MODE %q", mode)
	}
	loaded := os.Getenv("FAKE_LOADED")
	if loaded == "" {
		fail("FAKE_LOADED is not set")
	}
	maxFrame := envNumber("FAKE_MAX_FRAME")

	if _, err := readRequest(); err != nil {
		fail("reading load: %v", err)
	}
	if mode == "load_failed" {
		writeMessage(map[string]any{"type": "load_failed", "reason": "fakehelper: scripted load failure"})
		os.Exit(1)
	}
	writeFrame([]byte(loaded))
	if mode == "hang" {
		for {
			time.Sleep(time.Hour)
		}
	}
	for {
		request, err := readRequest()
		if errors.Is(err, io.EOF) {
			os.Exit(0)
		}
		if err != nil {
			fail("reading a request: %v", err)
		}
		switch request.Type {
		case "shutdown":
			os.Exit(0)
		case "callback":
			answer(mode, request.Seq, maxFrame)
		default:
			fail("unexpected %q request", request.Type)
		}
	}
}

// answer answers callback seq as mode scripts.
func answer(mode string, seq, maxFrame uint64) {
	switch mode {
	case "ok":
		fmt.Fprintf(os.Stderr, "callback %d\n", seq)
		commit(seq)
	case "env":
		var names []string
		for _, entry := range os.Environ() {
			name, _, _ := strings.Cut(entry, "=")
			names = append(names, name)
		}
		list, _ := json.Marshal(names)
		fmt.Fprintf(os.Stderr, "env %s\n", list)
		commit(seq)
	case "garbage":
		writeFrame([]byte("\x00garbage\xff"))
	case "oversized":
		writeLength(maxFrame + 1)
	case "exit":
		os.Exit(3)
	case "wrong_seq":
		commit(seq + 1)
	}
}

// commit answers callback seq with a committed result without ops.
func commit(seq uint64) {
	writeMessage(map[string]any{
		"type": "result", "seq": seq, "outcome": map[string]any{"type": "committed", "ops": []any{}},
	})
}

// readRequest reads one frame and returns its type and seq. A clean end of stdin is io.EOF.
func readRequest() (request struct {
	Type string `json:"type"`
	Seq  uint64 `json:"seq"`
}, err error) {
	var prefix [4]byte
	if _, err := io.ReadFull(os.Stdin, prefix[:]); err != nil {
		return request, err
	}
	body := make([]byte, binary.LittleEndian.Uint32(prefix[:]))
	if _, err := io.ReadFull(os.Stdin, body); err != nil {
		return request, err
	}
	return request, json.Unmarshal(body, &request)
}

func writeMessage(message any) {
	body, err := json.Marshal(message)
	if err != nil {
		fail("encoding %v: %v", message, err)
	}
	writeFrame(body)
}

func writeFrame(body []byte) {
	writeLength(uint64(len(body)))
	if _, err := os.Stdout.Write(body); err != nil {
		fail("writing a frame: %v", err)
	}
}

func writeLength(length uint64) {
	if _, err := os.Stdout.Write(binary.LittleEndian.AppendUint32(nil, uint32(length))); err != nil {
		fail("writing a frame length: %v", err)
	}
}

func envNumber(name string) uint64 {
	n, err := strconv.ParseUint(os.Getenv(name), 10, 32)
	if err != nil {
		fail("%s: %v", name, err)
	}
	return n
}

func fail(format string, args ...any) {
	fmt.Fprintf(os.Stderr, "fakehelper: "+format+"\n", args...)
	os.Exit(2)
}
