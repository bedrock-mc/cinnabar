package authcache

import (
	"errors"
	"fmt"
	"syscall"

	"github.com/df-mc/go-xsapi/v2/xal/sisu"
)

// errInvalidCache marks unusable token contents read from a trusted cache file.
var errInvalidCache = errors.New("invalid auth cache contents")

// cacheFailure retains an owned operation alongside the original error for safe diagnostics.
type cacheFailure struct {
	operation string
	err       error
}

func (e *cacheFailure) Error() string { return e.err.Error() }
func (e *cacheFailure) Unwrap() error { return e.err }

// FailureDiagnostic reports an owned operation and numeric OS error, if present.
// Unknown errors are never formatted because provider errors may contain credentials.
func FailureDiagnostic(err error) string {
	var failure *cacheFailure
	if !errors.As(err, &failure) {
		return "unclassified"
	}
	diagnostic := "cache_" + failure.operation
	var code syscall.Errno
	if errors.As(err, &code) {
		return fmt.Sprintf("%s:os_%d", diagnostic, code)
	}
	return diagnostic
}

// credentialError preserves actionable Xbox signup errors while replacing other provider errors
// with a message that cannot expose credentials or response contents.
func credentialError(err error, operation string) error {
	var signup *sisu.AccountCreationRequiredError
	if errors.As(err, &signup) {
		return signup
	}
	return errors.New("authentication: " + operation)
}
