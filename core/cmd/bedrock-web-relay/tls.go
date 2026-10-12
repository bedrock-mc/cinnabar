package main

import (
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"crypto/tls"
	"crypto/x509"
	"crypto/x509/pkix"
	"math/big"
	"net"
	"os"
	"time"
)

// selfSignedCertificate is a fresh certificate for this machine's names and addresses, so a
// development relay reached by LAN address is a secure context, which browsers require for
// WebGPU. Browsers ask the player to accept it once.
func selfSignedCertificate() (tls.Certificate, error) {
	key, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	if err != nil {
		return tls.Certificate{}, err
	}
	serial, err := rand.Int(rand.Reader, new(big.Int).Lsh(big.NewInt(1), 127))
	if err != nil {
		return tls.Certificate{}, err
	}
	template := &x509.Certificate{
		SerialNumber: serial,
		Subject:      pkix.Name{CommonName: "Cinnabar development relay"},
		NotBefore:    time.Now().Add(-time.Hour),
		NotAfter:     time.Now().Add(30 * 24 * time.Hour),
		KeyUsage:     x509.KeyUsageDigitalSignature,
		ExtKeyUsage:  []x509.ExtKeyUsage{x509.ExtKeyUsageServerAuth},
		DNSNames:     []string{"localhost"},
		IPAddresses:  localAddresses(),
	}
	if host, err := os.Hostname(); err == nil {
		template.DNSNames = append(template.DNSNames, host)
	}
	der, err := x509.CreateCertificate(rand.Reader, template, template, &key.PublicKey, key)
	if err != nil {
		return tls.Certificate{}, err
	}
	return tls.Certificate{Certificate: [][]byte{der}, PrivateKey: key}, nil
}

// localAddresses lists every address on this machine's interfaces.
func localAddresses() []net.IP {
	addresses := []net.IP{net.IPv4(127, 0, 0, 1), net.IPv6loopback}
	interfaces, err := net.InterfaceAddrs()
	if err != nil {
		return addresses
	}
	for _, address := range interfaces {
		if prefix, ok := address.(*net.IPNet); ok {
			addresses = append(addresses, prefix.IP)
		}
	}
	return addresses
}
