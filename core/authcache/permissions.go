package authcache

import "errors"

// errUnsafePermissions classifies a cache file whose on-disk permissions or
// ownership would let anyone other than the current user read or replace the
// token it holds.
var errUnsafePermissions = errors.New("auth cache file is not private to its owner")

// unsafePermissionsError reports one bounded, secret-free reason a cache file
// failed the private-file contract. The code is a fixed literal chosen at the
// rejection site so logs and tests can classify failures without inspecting
// token material.
type unsafePermissionsError struct {
	code   string
	detail string
}

func (e *unsafePermissionsError) Error() string {
	return "auth cache file is not private (" + e.code + "): " + e.detail
}

func (e *unsafePermissionsError) Unwrap() error {
	return errUnsafePermissions
}

func unsafePermissions(code, detail string) error {
	return &unsafePermissionsError{code: code, detail: detail}
}
