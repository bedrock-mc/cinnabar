package control

import "net"

// responseWriter binds the common JSON-RPC envelope to one request.
type responseWriter struct {
	server *Server
	conn   net.Conn
	id     uint64
}

// ok writes the method result using the shared response envelope.
func (r responseWriter) ok(result any) error {
	return r.server.writeResponse(r.conn, response{JSONRPC: "2.0", ID: r.id, Result: result})
}

// fail writes a public error without exposing the underlying service error.
func (r responseWriter) fail(code int, message string) error {
	return r.server.writeResponse(r.conn, response{JSONRPC: "2.0", ID: r.id, Error: &responseError{Code: code, Message: message}})
}

// invalid reports malformed method parameters.
func (r responseWriter) invalid() error {
	return r.fail(-32602, "Invalid params")
}
