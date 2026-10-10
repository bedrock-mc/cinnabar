package authcache

import (
	"errors"

	"github.com/df-mc/go-xsapi/v2/xal/sisu"
)

// credentialError preserves actionable Xbox signup errors while replacing other provider errors
// with a message that cannot expose credentials or response contents.
func credentialError(err error, operation string) error {
	var signup *sisu.AccountCreationRequiredError
	if errors.As(err, &signup) {
		return signup
	}
	return errors.New("authentication: " + operation)
}
