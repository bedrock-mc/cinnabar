package webrelay

import (
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"crypto/tls"
	"crypto/x509"
	"crypto/x509/pkix"
	"encoding/pem"
	"math/big"
	"net/netip"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestServeRejectsExpiredCertificateBeforeOpeningListeners(t *testing.T) {
	now := time.Now()
	directory := t.TempDir()
	cfg := ListenConfig{Hostname: "relay.invalid", UDPAddress: "invalid", TCPAddress: "invalid",
		CertificateFile: filepath.Join(directory, "cert.pem"), KeyFile: filepath.Join(directory, "key.pem")}
	writeCertificate(t, cfg, 1, cfg.Hostname, now.Add(-2*time.Hour), now.Add(-time.Hour))
	relay := New(Config{})
	defer relay.Close()
	err := relay.Serve(t.Context(), cfg)
	if err == nil || !strings.Contains(err.Error(), "validity period") {
		t.Fatalf("expired certificate must fail before listener creation: %v", err)
	}
}

// writeCertificate creates a local test pair with caller-controlled identity and validity.
func writeCertificate(t *testing.T, cfg ListenConfig, serial int64, host string, start, end time.Time) {
	t.Helper()
	key, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	template := &x509.Certificate{SerialNumber: big.NewInt(serial), Subject: pkix.Name{CommonName: host},
		DNSNames: []string{host}, NotBefore: start, NotAfter: end,
		KeyUsage: x509.KeyUsageDigitalSignature, ExtKeyUsage: []x509.ExtKeyUsage{x509.ExtKeyUsageServerAuth}}
	der, err := x509.CreateCertificate(rand.Reader, template, template, &key.PublicKey, key)
	if err != nil {
		t.Fatal(err)
	}
	private, err := x509.MarshalPKCS8PrivateKey(key)
	if err != nil {
		t.Fatal(err)
	}
	for path, block := range map[string]*pem.Block{
		cfg.CertificateFile: {Type: "CERTIFICATE", Bytes: der},
		cfg.KeyFile:         {Type: "PRIVATE KEY", Bytes: private},
	} {
		if err := os.WriteFile(path, pem.EncodeToMemory(block), 0600); err != nil {
			t.Fatal(err)
		}
	}
}

func TestCertificateReloadPublishesToBothListenersAndRetainsSessions(t *testing.T) {
	now := time.Unix(1_800_000_000, 0)
	directory := t.TempDir()
	cfg := ListenConfig{Hostname: "relay.invalid", CertificateFile: filepath.Join(directory, "cert.pem"), KeyFile: filepath.Join(directory, "key.pem")}
	writeCertificate(t, cfg, 1, cfg.Hostname, now.Add(-time.Hour), now.Add(time.Hour))
	certificates, err := newCertificateReloader(cfg, now)
	if err != nil {
		t.Fatal(err)
	}
	relay := New(Config{})
	defer relay.Close()
	controller, detach, err := relay.openControl(netip.MustParseAddr("203.0.113.1"), "", now)
	if err != nil {
		t.Fatal(err)
	}
	defer detach(now)
	base := &tls.Config{MinVersion: tls.VersionTLS13, GetCertificate: certificates.getCertificate}
	tcp := relay.socketServer(base, "")
	quic := relay.transportServer(base, "")
	old, _ := certificates.getCertificate(nil)
	writeCertificate(t, cfg, 2, cfg.Hostname, now.Add(-time.Hour), now.Add(2*time.Hour))
	if err := certificates.reload(now); err != nil {
		t.Fatal(err)
	}
	for _, config := range []*tls.Config{tcp.TLSConfig, quic.H3.TLSConfig} {
		certificate, err := config.GetCertificate(&tls.ClientHelloInfo{ServerName: cfg.Hostname})
		if err != nil || certificate.Leaf.SerialNumber.Int64() != 2 {
			t.Fatalf("listener did not receive renewed certificate: %v", err)
		}
	}
	if old.Leaf.SerialNumber.Int64() != 1 {
		t.Fatal("reload mutated a certificate used by an existing handshake")
	}
	if len(relay.sessions) != 1 {
		t.Fatal("reload lost the active session")
	}
	if _, retained := relay.sessions[controller.session]; !retained {
		t.Fatal("reload replaced the active session")
	}
}

func TestCertificateReloadRejectsBadReplacementsWithoutLosingGoodPair(t *testing.T) {
	now := time.Unix(1_800_000_000, 0)
	directory := t.TempDir()
	cfg := ListenConfig{Hostname: "relay.invalid", CertificateFile: filepath.Join(directory, "cert.pem"), KeyFile: filepath.Join(directory, "key.pem")}
	writeCertificate(t, cfg, 1, cfg.Hostname, now.Add(-time.Hour), now.Add(time.Hour))
	certificates, err := newCertificateReloader(cfg, now)
	if err != nil {
		t.Fatal(err)
	}
	good, _ := certificates.getCertificate(nil)
	for _, invalid := range []string{"hostname", "expired", "future", "key", "malformed"} {
		t.Run(invalid, func(t *testing.T) {
			host, start, end := cfg.Hostname, now.Add(-time.Hour), now.Add(time.Hour)
			switch invalid {
			case "hostname":
				host = "other.invalid"
			case "expired":
				end = now
			case "future":
				start = now.Add(time.Second)
			}
			writeCertificate(t, cfg, 2, host, start, end)
			if invalid == "key" || invalid == "malformed" {
				path := cfg.CertificateFile
				if invalid == "key" {
					path = cfg.KeyFile
				}
				if err := os.WriteFile(path, []byte("incomplete renewal"), 0600); err != nil {
					t.Fatal(err)
				}
			}
			if certificates.reload(now) == nil {
				t.Fatal("invalid replacement was accepted")
			}
			if _, err := newCertificateReloader(cfg, now); err == nil {
				t.Fatal("startup accepted an invalid certificate")
			}
			if current, _ := certificates.getCertificate(nil); current != good {
				t.Fatal("failed reload discarded the last good certificate")
			}
		})
	}
}
