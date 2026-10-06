package extension

import (
	"context"
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"crypto/tls"
	"crypto/x509"
	"crypto/x509/pkix"
	"encoding/pem"
	"errors"
	"fmt"
	"log/slog"
	"math/big"
	"net"
	"net/http"
	"net/netip"
	"os"
	"strings"
	"time"
)

// DefaultMediaAddr is where the media server listens unless -extension-media-addr says otherwise.
const DefaultMediaAddr = "127.0.0.1:19443"

// MediaCAFile is the file in the world directory that holds the media server's CA certificate, which
// a developer client trusts through CINNABAR_DEV_MEDIA_CA.
const MediaCAFile = "extension-media-ca.pem"

// certLifetime bounds the generated certificates; a restart issues new ones.
const certLifetime = 30 * 24 * time.Hour

// MediaOrigin is the canonical HTTPS origin of a loopback media server at addr, as the client
// serializes it. Only IPv4 loopback addresses with an explicit port are accepted.
func MediaOrigin(addr string) (string, error) {
	ap, err := netip.ParseAddrPort(addr)
	if err != nil || !ap.Addr().Is4() || !ap.Addr().IsLoopback() || ap.Port() == 0 {
		return "", fmt.Errorf("media address %q is not an IPv4 loopback ip:port such as %s", addr, DefaultMediaAddr)
	}
	if ap.Port() == 443 {
		return "https://" + ap.Addr().String(), nil
	}
	return "https://" + ap.String(), nil
}

// MediaServer serves the regular files of a directory over HTTPS on loopback, with byte ranges.
type MediaServer struct {
	srv  *http.Server
	ln   net.Listener
	root *os.Root
}

// ServeMedia starts serving dir at addr with a fresh CA and leaf certificate, writing the CA
// certificate as PEM to caPath before it accepts connections.
func ServeMedia(dir, addr, caPath string, log *slog.Logger) (*MediaServer, error) {
	ap, err := netip.ParseAddrPort(addr)
	if err != nil || !ap.Addr().Is4() || !ap.Addr().IsLoopback() {
		return nil, fmt.Errorf("media address %q is not an IPv4 loopback ip:port", addr)
	}
	root, err := os.OpenRoot(dir)
	if err != nil {
		return nil, fmt.Errorf("-extension-media: %w", err)
	}
	cert, caPEM, err := issue(net.IP(ap.Addr().AsSlice()))
	if err != nil {
		root.Close()
		return nil, err
	}
	if err := os.WriteFile(caPath, caPEM, 0o644); err != nil {
		root.Close()
		return nil, fmt.Errorf("write the media CA: %w", err)
	}
	ln, err := net.Listen("tcp", addr)
	if err != nil {
		root.Close()
		return nil, fmt.Errorf("listen for media: %w", err)
	}
	m := &MediaServer{
		srv: &http.Server{
			Handler:           mediaHandler(root),
			TLSConfig:         &tls.Config{Certificates: []tls.Certificate{cert}, MinVersion: tls.VersionTLS12},
			ReadHeaderTimeout: 10 * time.Second,
			ErrorLog:          slog.NewLogLogger(log.Handler(), slog.LevelDebug),
		},
		ln:   ln,
		root: root,
	}
	go func() {
		if err := m.srv.ServeTLS(ln, "", ""); err != nil && !errors.Is(err, http.ErrServerClosed) {
			log.Error("media server stopped", "err", err)
		}
	}()
	return m, nil
}

// Addr is the address the server listens on.
func (m *MediaServer) Addr() net.Addr {
	return m.ln.Addr()
}

// Close stops the server and releases the directory once in-flight requests finish, so the
// directory can be removed (Windows refuses while a handle is open).
func (m *MediaServer) Close() error {
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	err := m.srv.Shutdown(ctx)
	if err != nil {
		err = m.srv.Close()
	}
	return errors.Join(err, m.root.Close())
}

// mediaHandler serves GET and HEAD of regular files under root; directories and anything else
// are not found.
func mediaHandler(root *os.Root) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodGet && r.Method != http.MethodHead {
			http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
			return
		}
		name := strings.TrimPrefix(r.URL.Path, "/")
		file, err := root.Open(name)
		if err != nil {
			http.NotFound(w, r)
			return
		}
		defer file.Close()
		info, err := file.Stat()
		if err != nil || !info.Mode().IsRegular() {
			http.NotFound(w, r)
			return
		}
		http.ServeContent(w, r, info.Name(), info.ModTime(), file)
	})
}

// issue makes a CA and a leaf for ip that it signs, returning the leaf and the CA as PEM. The
// client's verifier refuses a CA certificate as the server's own, hence two.
func issue(ip net.IP) (tls.Certificate, []byte, error) {
	now := time.Now()
	caKey, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	if err != nil {
		return tls.Certificate{}, nil, err
	}
	ca := &x509.Certificate{
		SerialNumber:          serial(),
		Subject:               pkix.Name{CommonName: "Cinnabar local media CA"},
		NotBefore:             now.Add(-time.Hour),
		NotAfter:              now.Add(certLifetime),
		KeyUsage:              x509.KeyUsageCertSign | x509.KeyUsageDigitalSignature,
		BasicConstraintsValid: true,
		IsCA:                  true,
		MaxPathLenZero:        true,
	}
	caDER, err := x509.CreateCertificate(rand.Reader, ca, ca, &caKey.PublicKey, caKey)
	if err != nil {
		return tls.Certificate{}, nil, err
	}
	leafKey, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	if err != nil {
		return tls.Certificate{}, nil, err
	}
	leaf := &x509.Certificate{
		SerialNumber:          serial(),
		Subject:               pkix.Name{CommonName: ip.String()},
		NotBefore:             now.Add(-time.Hour),
		NotAfter:              now.Add(certLifetime),
		KeyUsage:              x509.KeyUsageDigitalSignature,
		ExtKeyUsage:           []x509.ExtKeyUsage{x509.ExtKeyUsageServerAuth},
		BasicConstraintsValid: true,
		IPAddresses:           []net.IP{ip},
	}
	leafDER, err := x509.CreateCertificate(rand.Reader, leaf, ca, &leafKey.PublicKey, caKey)
	if err != nil {
		return tls.Certificate{}, nil, err
	}
	cert := tls.Certificate{Certificate: [][]byte{leafDER, caDER}, PrivateKey: leafKey}
	return cert, pem.EncodeToMemory(&pem.Block{Type: "CERTIFICATE", Bytes: caDER}), nil
}

// serial is a random 128-bit certificate serial number.
func serial() *big.Int {
	n, err := rand.Int(rand.Reader, new(big.Int).Lsh(big.NewInt(1), 128))
	if err != nil {
		panic(err)
	}
	return n
}
