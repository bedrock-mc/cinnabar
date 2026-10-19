package coreapp

import (
	"errors"
	"unsafe"

	"golang.org/x/sys/windows"
)

// normalUser refuses inherited administrator rights before opening account data or services.
func normalUser() error {
	var elevated, length uint32
	if err := windows.GetTokenInformation(windows.GetCurrentProcessToken(), windows.TokenElevation,
		(*byte)(unsafe.Pointer(&elevated)), uint32(unsafe.Sizeof(elevated)), &length); err != nil {
		return err
	}
	if length != uint32(unsafe.Sizeof(elevated)) || elevated != 0 {
		return errors.New("the core cannot run as administrator; open the game normally from the Start menu")
	}
	return nil
}
