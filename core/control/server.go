package control

import (
	"bytes"
	"encoding/binary"
	"encoding/json"
	"errors"
	"io"
	"net"
	"sync"

	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
)

const MaxFrameLen = 64 * 1024

type request struct {
	JSONRPC string          `json:"jsonrpc"`
	ID      *uint64         `json:"id"`
	Method  string          `json:"method"`
	Params  json.RawMessage `json:"params,omitempty"`
}

type response struct {
	JSONRPC string         `json:"jsonrpc"`
	ID      any            `json:"id"`
	Result  *StatusV1      `json:"result,omitempty"`
	Error   *responseError `json:"error,omitempty"`
}

type responseError struct {
	Code    int    `json:"code"`
	Message string `json:"message"`
}

type Server struct {
	listener net.Listener
	store    *Store
	done     chan struct{}
	once     sync.Once
	mu       sync.Mutex
	active   net.Conn
	closing  bool
	err      error
}

// Start binds the distinct control endpoint before returning.
func Start(socketDir string, store *Store) (*Server, error) {
	if store == nil {
		return nil, errors.New("control: status store is required")
	}
	listener, err := streamnet.ListenControl(socketDir)
	if err != nil {
		return nil, err
	}
	server := &Server{listener: listener, store: store, done: make(chan struct{})}
	go server.serve()
	return server, nil
}

func (server *Server) serve() {
	defer close(server.done)
	for {
		conn, err := server.listener.Accept()
		if err != nil {
			if !errors.Is(err, net.ErrClosed) {
				server.mu.Lock()
				server.err = errors.Join(server.err, err)
				server.mu.Unlock()
			}
			return
		}
		server.mu.Lock()
		if server.closing {
			server.mu.Unlock()
			_ = conn.Close()
			return
		}
		server.active = conn
		server.mu.Unlock()
		_ = server.serveOne(conn)
		server.mu.Lock()
		if server.active == conn {
			server.active = nil
		}
		server.mu.Unlock()
		_ = conn.Close()
	}
}

func (server *Server) serveOne(conn net.Conn) error {
	payload, err := readFrame(conn)
	if err != nil {
		return err
	}
	if !json.Valid(payload) {
		return writeResponse(conn, response{JSONRPC: "2.0", ID: nil, Error: &responseError{Code: -32700, Message: "Parse error"}})
	}
	var call request
	decoder := json.NewDecoder(bytes.NewReader(payload))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&call); err != nil || decoder.Decode(new(any)) != io.EOF {
		return writeResponse(conn, response{JSONRPC: "2.0", ID: nil, Error: &responseError{Code: -32600, Message: "Invalid Request"}})
	}
	if call.JSONRPC != "2.0" || call.ID == nil || call.Method == "" {
		return writeResponse(conn, response{JSONRPC: "2.0", ID: call.ID, Error: &responseError{Code: -32600, Message: "Invalid Request"}})
	}
	id := *call.ID
	if len(call.Params) != 0 {
		return writeResponse(conn, response{JSONRPC: "2.0", ID: id, Error: &responseError{Code: -32602, Message: "Invalid params"}})
	}
	if call.Method != "status.v1" {
		return writeResponse(conn, response{JSONRPC: "2.0", ID: id, Error: &responseError{Code: -32601, Message: "Method not found"}})
	}
	status := server.store.Status()
	return writeResponse(conn, response{JSONRPC: "2.0", ID: id, Result: &status})
}

func (server *Server) Close() error {
	server.once.Do(func() {
		server.store.SetLifecycle(LifecycleStopping)
		server.mu.Lock()
		server.closing = true
		active := server.active
		server.mu.Unlock()
		closeErr := server.listener.Close()
		if active != nil {
			closeErr = errors.Join(closeErr, active.Close())
		}
		<-server.done
		server.mu.Lock()
		server.err = errors.Join(server.err, closeErr)
		server.mu.Unlock()
	})
	server.mu.Lock()
	err := server.err
	server.mu.Unlock()
	return err
}

func readFrame(reader io.Reader) ([]byte, error) {
	var header [4]byte
	if _, err := io.ReadFull(reader, header[:]); err != nil {
		return nil, err
	}
	length := binary.BigEndian.Uint32(header[:])
	if length == 0 || length > MaxFrameLen {
		return nil, errors.New("control: invalid frame length")
	}
	payload := make([]byte, length)
	_, err := io.ReadFull(reader, payload)
	return payload, err
}

func writeResponse(writer io.Writer, value response) error {
	payload, err := json.Marshal(value)
	if err != nil {
		return err
	}
	if len(payload) == 0 || len(payload) > MaxFrameLen {
		return errors.New("control: invalid response length")
	}
	var header [4]byte
	binary.BigEndian.PutUint32(header[:], uint32(len(payload)))
	if err := writeFull(writer, header[:]); err != nil {
		return err
	}
	return writeFull(writer, payload)
}

func writeFull(writer io.Writer, payload []byte) error {
	for len(payload) != 0 {
		n, err := writer.Write(payload)
		if err != nil {
			return err
		}
		if n == 0 {
			return io.ErrNoProgress
		}
		payload = payload[n:]
	}
	return nil
}
