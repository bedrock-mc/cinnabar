package control

import (
	"bytes"
	"encoding/binary"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"net"
	"sync"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
	"github.com/hashimthearab/rust-mcbe/core/proxy"
)

const (
	MaxFrameLen = 64 * 1024

	// requestIOTimeout bounds each read and write phase on the local, serial
	// status endpoint so one stalled tool cannot deny service to later clients.
	requestIOTimeout = 2 * time.Second
)

// maxConcurrentRequests bounds simultaneous local requests; extra connections are dropped.
const maxConcurrentRequests = 32

const methodPackApplication = "pack_application.v1"

type request struct {
	JSONRPC string          `json:"jsonrpc"`
	ID      *uint64         `json:"id"`
	Method  string          `json:"method"`
	Params  json.RawMessage `json:"params,omitempty"`
}

type response struct {
	JSONRPC string         `json:"jsonrpc"`
	ID      any            `json:"id"`
	Result  any            `json:"result,omitempty"`
	Error   *responseError `json:"error,omitempty"`
}

type responseError struct {
	Code    int    `json:"code"`
	Message string `json:"message"`
}

type Server struct {
	listener         net.Listener
	store            *Store
	worlds           Worlds      // nil disables the world_* methods; guarded by mu
	services         Services    // nil disables the launcher methods; guarded by mu
	marketplace      Marketplace // nil disables the store_* methods; guarded by mu
	packetDelay      *proxy.PacketDelay
	done             chan struct{}
	once             sync.Once
	mu               sync.Mutex
	active           map[net.Conn]struct{}
	handlers         sync.WaitGroup
	closing          bool
	err              error
	requestIOTimeout time.Duration
	logger           *slog.Logger // nil drops service-failure logs; guarded by mu
}

// Start binds the distinct control endpoint before returning.
func Start(socketDir string, store *Store) (*Server, error) {
	return startWithRequestIOTimeout(socketDir, store, requestIOTimeout)
}

// StartWithWorlds is Start plus the versioned world_* methods backed by worlds.
func StartWithWorlds(socketDir string, store *Store, worlds Worlds) (*Server, error) {
	return startServer(socketDir, store, worlds, requestIOTimeout)
}

func startWithRequestIOTimeout(socketDir string, store *Store, timeout time.Duration) (*Server, error) {
	return startServer(socketDir, store, nil, timeout)
}

func startServer(socketDir string, store *Store, worlds Worlds, timeout time.Duration) (*Server, error) {
	if store == nil {
		return nil, errors.New("control: status store is required")
	}
	if timeout <= 0 {
		return nil, errors.New("control: request I/O timeout must be positive")
	}
	listener, err := streamnet.ListenControl(socketDir)
	if err != nil {
		return nil, err
	}
	server := &Server{
		listener:         listener,
		store:            store,
		worlds:           worlds,
		active:           make(map[net.Conn]struct{}),
		done:             make(chan struct{}),
		requestIOTimeout: timeout,
	}
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
		if len(server.active) >= maxConcurrentRequests {
			server.mu.Unlock()
			_ = conn.Close()
			continue
		}
		server.active[conn] = struct{}{}
		server.handlers.Add(1)
		server.mu.Unlock()
		go func() {
			defer server.handlers.Done()
			_ = server.serveOne(conn)
			server.mu.Lock()
			delete(server.active, conn)
			server.mu.Unlock()
			_ = conn.Close()
		}()
	}
}

// SetWorlds enables the world_* methods; safe to call while serving.
func (server *Server) SetWorlds(worlds Worlds) {
	server.mu.Lock()
	server.worlds = worlds
	server.mu.Unlock()
}

// SetServices enables the launcher methods; safe to call while serving.
// SetLogger records failed launcher service calls, their errors redacted.
func (server *Server) SetLogger(logger *slog.Logger) {
	server.mu.Lock()
	server.logger = logger
	server.mu.Unlock()
}

func (server *Server) logServiceFailure(method string, err error) {
	server.mu.Lock()
	logger := server.logger
	server.mu.Unlock()
	if logger != nil {
		logger.Warn("launcher service failed", "method", method, "error", RedactError(err))
	}
}

func (server *Server) SetServices(services Services) {
	server.mu.Lock()
	server.services = services
	server.mu.Unlock()
}

func (server *Server) worldService() Worlds {
	server.mu.Lock()
	defer server.mu.Unlock()
	return server.worlds
}

func (server *Server) launcherServices() Services {
	server.mu.Lock()
	defer server.mu.Unlock()
	return server.services
}

func (server *Server) serveOne(conn net.Conn) error {
	if err := conn.SetReadDeadline(time.Now().Add(server.requestIOTimeout)); err != nil {
		return fmt.Errorf("control: set request read deadline: %w", err)
	}
	payload, err := readFrame(conn)
	if err != nil {
		return err
	}
	if !json.Valid(payload) {
		return server.writeResponse(conn, response{JSONRPC: "2.0", ID: nil, Error: &responseError{Code: -32700, Message: "Parse error"}})
	}
	var call request
	decoder := json.NewDecoder(bytes.NewReader(payload))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&call); err != nil || decoder.Decode(new(any)) != io.EOF {
		return server.writeResponse(conn, response{JSONRPC: "2.0", ID: nil, Error: &responseError{Code: -32600, Message: "Invalid Request"}})
	}
	if call.JSONRPC != "2.0" || call.ID == nil || call.Method == "" {
		return server.writeResponse(conn, response{JSONRPC: "2.0", ID: call.ID, Error: &responseError{Code: -32600, Message: "Invalid Request"}})
	}
	id := *call.ID
	if call.Method == methodPacketDelay {
		return server.servePacketDelay(conn, id, call.Params)
	}
	if call.Method == methodPackApplication {
		return server.servePackApplication(conn, id, call.Params)
	}
	if isServiceMethod(call.Method) {
		return server.serveService(conn, id, call.Method, call.Params)
	}
	if isStoreMethod(call.Method) {
		return server.serveStore(conn, id, call.Method, call.Params)
	}
	if server.worldService() != nil && isWorldMethod(call.Method) {
		return server.serveWorld(conn, id, call.Method, call.Params)
	}
	if len(call.Params) != 0 {
		return server.writeResponse(conn, response{JSONRPC: "2.0", ID: id, Error: &responseError{Code: -32602, Message: "Invalid params"}})
	}
	if call.Method != "status.v1" {
		return server.writeResponse(conn, response{JSONRPC: "2.0", ID: id, Error: &responseError{Code: -32601, Message: "Method not found"}})
	}
	status := server.store.Status()
	return server.writeResponse(conn, response{JSONRPC: "2.0", ID: id, Result: &status})
}

// servePackApplication records whether the client applied an attempt's packs
// and answers with the resulting status.
func (server *Server) servePackApplication(conn net.Conn, id uint64, raw json.RawMessage) error {
	var params struct {
		AttemptID *uint64 `json:"attempt_id"`
		Applied   *bool   `json:"applied"`
	}
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&params); err != nil || decoder.Decode(new(any)) != io.EOF || params.AttemptID == nil || params.Applied == nil {
		return server.writeResponse(conn, response{JSONRPC: "2.0", ID: id, Error: &responseError{Code: -32602, Message: "Invalid params"}})
	}
	server.store.SetApplied(*params.AttemptID, *params.Applied)
	status := server.store.Status()
	return server.writeResponse(conn, response{JSONRPC: "2.0", ID: id, Result: &status})
}

func (server *Server) writeResponse(conn net.Conn, value any) error {
	if err := conn.SetWriteDeadline(time.Now().Add(server.requestIOTimeout)); err != nil {
		return fmt.Errorf("control: set response write deadline: %w", err)
	}
	return writeResponse(conn, value)
}

func (server *Server) Close() error {
	server.once.Do(func() {
		server.store.SetLifecycle(LifecycleStopping)
		server.mu.Lock()
		server.closing = true
		active := make([]net.Conn, 0, len(server.active))
		for conn := range server.active {
			active = append(active, conn)
		}
		server.mu.Unlock()
		closeErr := server.listener.Close()
		for _, conn := range active {
			closeErr = errors.Join(closeErr, conn.Close())
		}
		<-server.done
		server.handlers.Wait()
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

func writeResponse(writer io.Writer, value any) error {
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
