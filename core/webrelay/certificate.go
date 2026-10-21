package webrelay

import (
	"crypto/tls"
	"crypto/x509"
	"errors"
	"sync/atomic"
	"time"
)

// certificateReloader publishes immutable certificates shared by the TCP and QUIC listeners.
type certificateReloader struct {
	cfg     ListenConfig
	current atomic.Pointer[tls.Certificate]
}

// newCertificateReloader requires a usable initial certificate before listeners can start.
func newCertificateReloader(cfg ListenConfig, now time.Time) (*certificateReloader, error) {
	certificates := &certificateReloader{cfg: cfg}
	if err := certificates.reload(now); err != nil {
		return nil, err
	}
	return certificates, nil
}

// reload validates the replacement before publishing it; failures retain the last good pair.
func (certificates *certificateReloader) reload(now time.Time) error {
	certificate, err := tls.LoadX509KeyPair(certificates.cfg.CertificateFile, certificates.cfg.KeyFile)
	if err != nil {
		return err
	}
	leaf, err := x509.ParseCertificate(certificate.Certificate[0])
	if err != nil {
		return err
	}
	if err := leaf.VerifyHostname(certificates.cfg.Hostname); err != nil {
		return err
	}
	if now.Before(leaf.NotBefore) || !now.Before(leaf.NotAfter) {
		return errors.New("webrelay: certificate is outside its validity period")
	}
	certificate.Leaf = leaf
	certificates.current.Store(&certificate)
	return nil
}

// getCertificate reads the latest validated pair without disk I/O on the handshake path.
func (certificates *certificateReloader) getCertificate(*tls.ClientHelloInfo) (*tls.Certificate, error) {
	return certificates.current.Load(), nil
}
