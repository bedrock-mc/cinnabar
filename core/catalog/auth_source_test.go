package catalog

import (
	"context"
	"crypto/ecdsa"
	"testing"

	"github.com/df-mc/go-xsapi/v2/xal/xasd"
	"github.com/df-mc/go-xsapi/v2/xal/xsts"
	"golang.org/x/oauth2"
)

func TestXSAPITokenSourcePreservesDerivedCache(t *testing.T) {
	source := newTestCompoundSource()
	if got := xsapiTokenSource(source); got != source {
		t.Fatal("derived Xbox token source was replaced")
	}
}

type testCompoundSource struct {
	token *oauth2.Token
	key   *ecdsa.PrivateKey
}

func newTestCompoundSource() *testCompoundSource {
	return &testCompoundSource{token: &oauth2.Token{AccessToken: "synthetic"}}
}

func (s *testCompoundSource) Token() (*oauth2.Token, error)                    { return s.token, nil }
func (s *testCompoundSource) DeviceToken(context.Context) (*xasd.Token, error) { return nil, nil }
func (s *testCompoundSource) ProofKey() *ecdsa.PrivateKey                      { return s.key }
func (s *testCompoundSource) XSTSToken(context.Context, string) (*xsts.Token, error) {
	return nil, nil
}
