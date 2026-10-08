package main

import (
	"errors"
	"golang.org/x/sys/windows"
)

// parentAlive checks a synchronize handle so access failures never permit installation.
func parentAlive(pid int) (bool, error) {
	process, err := windows.OpenProcess(windows.SYNCHRONIZE, false, uint32(pid))
	if errors.Is(err, windows.ERROR_INVALID_PARAMETER) {
		return false, nil
	}
	if err != nil {
		return false, err
	}
	defer windows.CloseHandle(process)
	result, err := windows.WaitForSingleObject(process, 0)
	if err != nil {
		return false, err
	}
	return result != windows.WAIT_OBJECT_0, nil
}
